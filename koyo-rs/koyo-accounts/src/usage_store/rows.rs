//! Part of the port of claude-swap's `usage_store.py` (0fe22a8): the rules over one stored
//! row, under the store's lock or on its lock-free read: a row read as its entry (`entries`'
//! body, with the trust rules), an outcome written into it (`record`'s `apply`), whether it
//! may be fetched now (`_row_eligible`), and, Koyo's own, a live reading laid over its last
//! good one (`lay_over`, for `UsageStore::adopt_over`).

use serde_json::{Map, Value};

use super::backoff::{
    PERMANENT_AUTH_ERRORS, TRUST_MAX_AGE_S, failure_backoff_s, rate_limited_trust_ok,
    rejection_backoff_until,
};
use super::{FetchRecord, PollPlan, UsageEntry, live_claim};
use crate::oauth::value::{num_or_none, py_int, py_number};

pub(super) type Row = Map<String, Value>;

pub(super) fn opt(value: Option<f64>) -> Value {
    value.map_or(Value::Null, Value::from)
}

fn text(row: &Row, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// One stored row as its entry at `now`.
pub(super) fn entry_of(row: &Row, now: f64, models: &[&str]) -> UsageEntry {
    let fetched_at = row.get("fetchedAt").and_then(py_number);
    let last_good = row.get("lastGood").filter(|v| v.is_object()).cloned();
    let age_s = fetched_at.map(|at| now - at);
    let consecutive_failures = py_int(row.get("consecutiveFailures"));
    let next_poll_at = num_or_none(row.get("nextPollAt"));
    let last_attempt_at = num_or_none(row.get("lastAttemptAt"));
    let claim_until = num_or_none(row.get("claimUntil"));
    let held_until = num_or_none(row.get("heldUntil"));
    // A 429 throttles polling without moving the account's windows, so the reading is trusted
    // to its earliest reset; any other failure is no evidence it holds, and takes the general
    // ceiling. Strict `<` on `nextPollAt` as `due_candidate`: at it the entry is due. A live
    // claim keeps the bridge up while another collector's result is in flight; a live hold too.
    let within_ceiling =
        if row.get("lastError").and_then(Value::as_str) == Some(crate::oauth::RATE_LIMITED_ERROR) {
            rate_limited_trust_ok(last_good.as_ref(), age_s, now, models)
        } else {
            age_s.is_some_and(|age| age <= TRUST_MAX_AGE_S)
        };
    let trust_extended = within_ceiling
        && (consecutive_failures > 0
            || next_poll_at.is_some_and(|at| now < at)
            || live_claim(claim_until, last_attempt_at, now)
            || held_until.is_some_and(|until| now < until));
    UsageEntry {
        sentinel: None,
        last_good,
        fetched_at,
        age_s,
        last_attempt_at,
        consecutive_failures,
        last_error: text(row, "lastError"),
        backoff_until: num_or_none(row.get("backoffUntil")),
        next_poll_at,
        poll_interval_s: num_or_none(row.get("pollIntervalS")),
        last_429_at: num_or_none(row.get("last429At")),
        auth_dead_strikes: py_int(row.get("authDeadStrikes")),
        struck_fingerprint: text(row, "struckFingerprint"),
        rejected_fingerprint: text(row, "rejectedFingerprint"),
        trust_extended,
        claim_until,
        held_until,
    }
}

/// One outcome written into its row (`record`'s `apply`).
pub(super) fn apply_outcome(
    row: &mut Row,
    outcome: &FetchRecord,
    plan: Option<&PollPlan>,
    now: f64,
) {
    row.insert("claimId".to_owned(), Value::Null);
    row.insert("claimUntil".to_owned(), Value::from(0.));
    if outcome.sentinel.is_some() {
        if let Some(stamp) = &outcome.rejected_fp {
            row.insert("rejectedFingerprint".to_owned(), Value::from(stamp.clone()));
        }
        return;
    }
    row.insert("lastAttemptAt".to_owned(), Value::from(now));
    let Some(error) = &outcome.error else {
        row.insert(
            "lastGood".to_owned(),
            outcome.usage.clone().unwrap_or(Value::Null),
        );
        row.insert("fetchedAt".to_owned(), Value::from(now));
        // The old, perhaps due, plan is replaced in the same transaction, so no collector
        // slips into a record-then-replan gap.
        if let Some((next_poll_at, interval)) = plan {
            row.insert("nextPollAt".to_owned(), opt(*next_poll_at));
            row.insert("pollIntervalS".to_owned(), opt(*interval));
        }
        row.insert("consecutiveFailures".to_owned(), Value::from(0));
        row.insert("lastError".to_owned(), Value::Null);
        // A confirmed refusal's Retry-After is a deadline no collector may fetch before.
        let backoff = rejection_backoff_until(outcome.usage.as_ref(), now);
        row.insert("backoffUntil".to_owned(), opt(backoff));
        row.insert("rejectedFingerprint".to_owned(), Value::Null);
        // A success proves the token alive.
        row.insert("authDeadStrikes".to_owned(), Value::from(0));
        return;
    };
    let failures = py_int(row.get("consecutiveFailures")).saturating_add(1);
    row.insert("consecutiveFailures".to_owned(), Value::from(failures));
    row.insert("lastError".to_owned(), Value::from(error.clone()));
    let rate_limited = error == crate::oauth::RATE_LIMITED_ERROR;
    if rate_limited {
        // Kept across later successes: the planner floors the cadence while it is recent.
        row.insert("last429At".to_owned(), Value::from(now));
    }
    let wait = failure_backoff_s(failures, outcome.retry_after_s, rate_limited);
    row.insert("backoffUntil".to_owned(), Value::from(now + wait));
    // Only a permanent-auth failure strikes; a transient one is no evidence either way and
    // must not reset a real tally.
    if PERMANENT_AUTH_ERRORS.contains(&error.as_str()) {
        let strikes = py_int(row.get("authDeadStrikes")).saturating_add(1);
        row.insert("authDeadStrikes".to_owned(), Value::from(strikes));
        // Always overwritten: a legacy writer's strike binds unconditionally rather than
        // inheriting the fingerprint of an earlier, healed strike.
        row.insert(
            "struckFingerprint".to_owned(),
            outcome.struck_fp.clone().map_or(Value::Null, Value::from),
        );
    }
}

/// Whether a stored row may be fetched now, under the write lock ([`super::UsageStore::reserve`]
/// says what the two modes mean): [`UsageEntry::eligible`] of the fields it reads.
pub fn row_eligible(row: &Row, now: f64, respect_plans: bool, repair_overslept: bool) -> bool {
    UsageEntry {
        auth_dead_strikes: py_int(row.get("authDeadStrikes")),
        backoff_until: num_or_none(row.get("backoffUntil")),
        held_until: num_or_none(row.get("heldUntil")),
        claim_until: num_or_none(row.get("claimUntil")),
        last_attempt_at: num_or_none(row.get("lastAttemptAt")),
        fetched_at: num_or_none(row.get("fetchedAt")),
        next_poll_at: num_or_none(row.get("nextPollAt")),
        poll_interval_s: num_or_none(row.get("pollIntervalS")),
        ..UsageEntry::default()
    }
    .eligible(now, respect_plans, repair_overslept)
}

/// A live reading laid over the stored one: each window `live` carries replaces the stored one
/// (a scoped entry by its name); everything else stored stays, so a reading that didn't carry a
/// window doesn't erase it.
pub(super) fn lay_over(stored: Option<&Value>, live: &Map<String, Value>) -> Value {
    let mut merged = stored
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in ["five_hour", "seven_day"] {
        if let Some(window) = live.get(key) {
            merged.insert(key.to_owned(), window.clone());
        }
    }
    if let Some(Value::Array(live_scoped)) = live.get("scoped") {
        let mut scoped = match merged.remove("scoped") {
            Some(Value::Array(scoped)) => scoped,
            _ => Vec::new(),
        };
        for entry in live_scoped {
            match scoped
                .iter_mut()
                .find(|stored| stored.get("name") == entry.get("name"))
            {
                Some(stored) => stored.clone_from(entry),
                None => scoped.push(entry.clone()),
            }
        }
        merged.insert("scoped".to_owned(), Value::Array(scoped));
    }
    Value::Object(merged)
}
