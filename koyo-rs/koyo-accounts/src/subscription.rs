//! Port of claude-swap's `subscription.py` (0fe22a8): each account's subscription plan, for
//! display only (#350).
//!
//! The plan comes from `GET /api/oauth/profile`, the endpoint `oauth::fetch_oauth_profile`
//! reads identity from; five `organization` fields are kept. Measured shapes, one account each:
//!
//! | state                 | status   | `rate_limit_tier`        | `billing_type`        |
//! |-----------------------|----------|--------------------------|-----------------------|
//! | active Max            | active   | `default_claude_max_20x` | `stripe_subscription` |
//! | cancelled, in period  | active   | `default_claude_max_20x` | `stripe_subscription` |
//! | lapsed                | canceled | `default_claude_ai`      | none                  |
//!
//! 2026-11-07: an Ultra account carries `default_claude_max_20x`, organization type
//! `claude_max` and plan name "Ultra", so the tier alone doesn't name the plan; an Enterprise
//! seat carries `default_claude_zero` and no name, so its organization type names it.
//! `subscription_status` moves only at lapse, not at cancellation, so renewal isn't shown.
//! Expired is `canceled` together with the free tier.
//!
//! Paced on its own clock and stored in its own file (`cache/subscription_plans.json` under
//! Koyo's store, as cswap's beside its usage cache), never through the usage store: a plan read
//! riding on the rate-limited usage endpoint would freeze an old label behind exactly the
//! accounts whose calls fail. A read only uses an access token that is already valid; this
//! module never refreshes one. A response is believed only once its account and organization
//! UUIDs match the slot; a failed or mismatched read sets `retryAt` and leaves `fetchedAt`, so
//! the label keeps its age and the endpoint isn't asked on every pass.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::http::Request;
use crate::locking::{DEFAULT_TIMEOUT, FileLock};
use crate::oauth::value::{number, str_at};
use crate::oauth::{
    FetchError, PROFILE_URL, USER_AGENT, classify_usage_error, extract_oauth_data,
    is_oauth_token_expired,
};
use crate::{Env, Result, fsutil, paths};

pub const PLAN_FILENAME: &str = "subscription_plans.json";

/// Plans change on a billing cadence: six hours shows a lapse the same day for four profile
/// calls an account a day.
pub const PLAN_TTL_S: f64 = 6. * 3600.;

/// After a failed read, at least this, longer if the server asks: `Retry-After: 0` is real on
/// this API (#220), so it can't be the only floor.
pub const FAILURE_RETRY_S: f64 = 15. * 60.;
/// The ceiling on any wait, and the bound past which a stored `retryAt` is clock skew.
pub const MAX_RETRY_S: f64 = 24. * 3600.;

pub const FREE_TIER: &str = "default_claude_ai";

/// The profile read's bound (cswap's `timeout=5.0`).
pub const PLAN_TIMEOUT: Duration = Duration::from_secs(5);

/// Only tiers actually observed; an unknown one shows raw rather than a guessed label.
const TIER_LABELS: [(&str, &str); 3] = [
    ("default_claude_ai", "Free"),
    ("default_claude_max_5x", "Max 5x"),
    ("default_claude_max_20x", "Max 20x"),
];

/// A Team or Enterprise seat isn't a personal subscription whatever its tier, and a paying Pro
/// subscription reports the default bucket (`default_claude_ai`, as a lapsed account), so the
/// organization type wins where it's known.
const ORG_TYPE_LABELS: [(&str, &str); 3] = [
    ("claude_team", "Team"),
    ("claude_enterprise", "Enterprise"),
    ("claude_pro", "Pro"),
];

const KEPT_FIELDS: [(&str, &str); 4] = [
    ("organization_type", "organizationType"),
    ("billing_type", "billingType"),
    ("subscription_status", "subscriptionStatus"),
    ("plan_display_name", "planDisplayName"),
];

fn lookup(table: &[(&str, &'static str)], key: Option<&str>) -> Option<&'static str> {
    let key = key?;
    table
        .iter()
        .find(|(from, _)| *from == key)
        .map(|(_, to)| *to)
}

/// One profile read; `plan` is `None` on any failure.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanFetch {
    pub plan: Option<Map<String, Value>>,
    pub account_uuid: String,
    pub organization_uuid: String,
    pub retry_after_s: Option<f64>,
}

/// The plan fields of `/api/oauth/profile`. A non-empty `rate_limit_tier` is required, the field
/// every label derives from: without one the answer is no answer. Never called with a credential
/// or config lock held.
pub fn fetch_plan(env: &Env, access_token: &str, timeout: Duration) -> PlanFetch {
    let request = Request::get(PROFILE_URL, timeout)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .header("User-Agent", USER_AGENT);
    let failed = |error: FetchError| {
        log::debug!("Plan fetch failed: {error}");
        PlanFetch {
            retry_after_s: classify_usage_error(&error).1,
            ..PlanFetch::default()
        }
    };
    let response = match env.http.send(&request) {
        Ok(response) if (200..300).contains(&response.status) => response,
        Ok(response) => return failed(FetchError::Status(response)),
        Err(error) => return failed(FetchError::Transport(error)),
    };
    let Ok(data) = serde_json::from_slice::<Value>(&response.body) else {
        return failed(FetchError::BadResponse("unreadable plan".into()));
    };
    let Some(organization) = data.get("organization").and_then(Value::as_object) else {
        return PlanFetch::default();
    };
    let Some(tier) = str_at(organization, "rate_limit_tier").filter(|tier| !tier.is_empty()) else {
        return PlanFetch::default();
    };
    let mut plan = Map::new();
    plan.insert("tier".to_owned(), Value::from(tier));
    for (source, key) in KEPT_FIELDS {
        if let Some(value) = str_at(organization, source).filter(|value| !value.is_empty()) {
            plan.insert(key.to_owned(), Value::from(value));
        }
    }
    // Always present (null if unsent): `is_due` reads a missing key as an old record.
    plan.entry("planDisplayName").or_insert(Value::Null);
    let account_uuid = data
        .get("account")
        .and_then(Value::as_object)
        .and_then(|account| str_at(account, "uuid"));
    PlanFetch {
        plan: Some(plan),
        account_uuid: account_uuid.unwrap_or_default().to_owned(),
        organization_uuid: str_at(organization, "uuid").unwrap_or_default().to_owned(),
        retry_after_s: None,
    }
}

/// Whether a read provably belongs to the slot it was made for: every UUID the slot records
/// matches, and at least one is recorded (a slot with neither shows no plan rather than perhaps
/// another account's).
pub fn matches_slot(fetched: &PlanFetch, account_uuid: &str, org_uuid: &str) -> bool {
    let expected: Vec<(&str, &str)> = [
        (account_uuid, fetched.account_uuid.as_str()),
        (org_uuid, fetched.organization_uuid.as_str()),
    ]
    .into_iter()
    .filter(|(recorded, _)| !recorded.is_empty())
    .collect();
    !expected.is_empty() && expected.iter().all(|(recorded, seen)| recorded == seen)
}

impl PlanFetch {
    /// The update this read saves (see [`merge`]), for the slot recording `account_uuid` and
    /// `org_uuid`: a success's plan stamped `started_at`, when the request went out, if the
    /// read [`matches_slot`]; otherwise a failure's `retryAt`, which also stops a response for
    /// another account from being kept.
    pub fn update(self, started_at: f64, account_uuid: &str, org_uuid: &str) -> Value {
        let mine = matches_slot(&self, account_uuid, org_uuid);
        let mut update = Map::new();
        match self.plan {
            Some(plan) if mine => {
                update.insert("plan".to_owned(), Value::Object(plan));
                update.insert("fetchedAt".to_owned(), Value::from(started_at));
            }
            _ => {
                update.insert(
                    "retryAt".to_owned(),
                    Value::from(next_retry_at(started_at, self.retry_after_s)),
                );
                update.insert("attemptedAt".to_owned(), Value::from(started_at));
            }
        }
        Value::Object(update)
    }
}

/// When a failed read may be repeated.
pub fn next_retry_at(now: f64, retry_after_s: Option<f64>) -> f64 {
    let wait = FAILURE_RETRY_S.max(retry_after_s.unwrap_or(0.));
    now + wait.min(MAX_RETRY_S)
}

/// The label: the organization type where known, else the server's plan name, else the tier's
/// label, else the raw tier.
pub fn plan_label(plan: Option<&Map<String, Value>>) -> Option<String> {
    let plan = plan.filter(|plan| !plan.is_empty())?;
    if let Some(label) = lookup(&ORG_TYPE_LABELS, str_at(plan, "organizationType")) {
        return Some(label.to_owned());
    }
    let tier = str_at(plan, "tier");
    let label = lookup(&TIER_LABELS, tier).or(tier);
    // The server's name wins unless the table names the plan more precisely ("Max" against
    // "Max 5x"); Ultra shares the Max 20x tier.
    if let Some(name) = str_at(plan, "planDisplayName").filter(|name| !name.is_empty())
        && !label.is_some_and(|label| label.starts_with(name))
    {
        return Some(name.to_owned());
    }
    label.map(str::to_owned)
}

/// A lapsed subscription: `canceled` and back on the free tier.
pub fn is_expired(plan: Option<&Map<String, Value>>) -> bool {
    plan.filter(|plan| !plan.is_empty()).is_some_and(|plan| {
        str_at(plan, "subscriptionStatus") == Some("canceled")
            && str_at(plan, "tier") == Some(FREE_TIER)
    })
}

/// The store key: identity, not slot number (slots get renumbered).
pub fn identity_key(email: &str, org_uuid: Option<&str>) -> String {
    format!("{email}\n{}", org_uuid.unwrap_or_default())
}

/// The access token of `credentials` when present and unexpired.
pub fn usable_access_token(credentials: Option<&str>, now: f64) -> Option<String> {
    let oauth = extract_oauth_data(credentials.filter(|c| !c.is_empty())?)?;
    let token = str_at(&oauth, "accessToken").filter(|token| !token.is_empty())?;
    (!is_oauth_token_expired(oauth.get("expiresAt"), now)).then(|| token.to_owned())
}

pub fn valid_record(record: &Value) -> bool {
    record
        .get("plan")
        .and_then(Value::as_object)
        .is_some_and(|plan| plan.get("tier").is_some_and(Value::is_string))
        && record.get("fetchedAt").and_then(number).is_some()
}

/// Whether a record needs refreshing; a stamp ahead of `now` is clock skew, so refresh. A live
/// `retryAt` holds the read back whatever the plan's age; one further out than any wait set here
/// is skew and ignored.
pub fn is_due(record: Option<&Value>, now: f64, ttl: f64) -> bool {
    let retry_at = record.and_then(|r| r.get("retryAt")).and_then(number);
    if retry_at.is_some_and(|at| now < at && at <= now + MAX_RETRY_S) {
        return false;
    }
    let Some(record) = record.filter(|record| valid_record(record)) else {
        return true;
    };
    // Stored before the name was kept.
    if record
        .get("plan")
        .is_none_or(|plan| plan.get("planDisplayName").is_none())
    {
        return true;
    }
    let fetched_at = record.get("fetchedAt").and_then(number).unwrap_or(0.);
    fetched_at > now || now - fetched_at >= ttl
}

/// A stored stamp that may take part in ordering: a number no later than `now`. One ahead of
/// the clock was written before the clock was set back and says nothing about recency.
fn past_stamp(record: &Map<String, Value>, key: &str, now: f64) -> Option<f64> {
    record.get(key).and_then(number).filter(|at| *at <= now)
}

/// One key of a save: the newer reading wins, whichever process lands last.
///
/// `update` is a success (`plan`, `fetchedAt`) or a failure (`retryAt`, `attemptedAt`), both
/// stamped before their request went out, so a slow answer can't pass for a newer one and an
/// older failure can't shorten a newer one's wait. A stored stamp ahead of `now` never outranks
/// the update: `is_due` asks for a fresh read of such a record, and it must be allowed to land.
pub fn merge(existing: Option<&Value>, update: &Map<String, Value>, now: f64) -> Value {
    let current = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let fetched = existing
        .filter(|record| valid_record(record))
        .and_then(|_| past_stamp(&current, "fetchedAt", now));
    let attempted = past_stamp(&current, "attemptedAt", now);
    let stamp = |key: &str| update.get(key).and_then(number).unwrap_or(0.);
    if update.contains_key("plan") {
        let landed = stamp("fetchedAt");
        if fetched.is_some_and(|fetched| fetched > landed) {
            return Value::Object(current);
        }
        if let Some(attempted) = attempted.filter(|attempted| *attempted > landed) {
            let mut kept = update.clone();
            kept.insert(
                "retryAt".to_owned(),
                current.get("retryAt").cloned().unwrap_or(Value::Null),
            );
            kept.insert("attemptedAt".to_owned(), Value::from(attempted));
            return Value::Object(kept);
        }
        return Value::Object(update.clone());
    }
    let tried = stamp("attemptedAt");
    if fetched.is_some_and(|fetched| fetched >= tried)
        || attempted.is_some_and(|attempted| attempted > tried)
    {
        return Value::Object(current);
    }
    let mut merged = current;
    merged.extend(update.clone());
    Value::Object(merged)
}

/// `subscription_plans.json`: identity key → record. `save` holds a lock across its read,
/// merge and write, and only that: the network calls come before it.
pub struct PlanStore {
    env: Env,
    pub path: PathBuf,
}

impl PlanStore {
    /// The plan file of `env`'s store, beside the usage table.
    pub fn new(env: &Env) -> PlanStore {
        PlanStore {
            env: env.clone(),
            path: paths::store::cache_dir(env).join(PLAN_FILENAME),
        }
    }

    pub fn load(&self) -> Map<String, Value> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|raw| match raw {
                Value::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Merges `updates` (key → a success or failure record) into the file.
    ///
    /// # Errors
    /// The lock stayed held, or the file couldn't be written.
    pub fn save(&self, updates: &Map<String, Value>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fsutil::create_dir_all(&self.env, parent)?;
        }
        let mut lock_name = self.path.as_os_str().to_owned();
        lock_name.push(".lock");
        let _lock = FileLock::hold(&self.env, &PathBuf::from(lock_name), DEFAULT_TIMEOUT)?;
        // The clock is read under the lock: a record another writer landed while this one
        // waited carries a later stamp than a clock read before the wait, and would be taken
        // for skew and overwritten.
        let now = self.env.now();
        let mut merged = self.load();
        for (key, update) in updates {
            let Some(update) = update.as_object() else {
                continue;
            };
            let record = merge(merged.get(key), update, now);
            merged.insert(key.clone(), record);
        }
        fsutil::atomic_write_json(&self.env, &self.path, &Value::Object(merged))
    }
}

#[cfg(test)]
mod tests;
