//! Port of claude-swap's `usage_store.py` (0fe22a8): per account, the last good measurement and
//! the fetch and backoff state, in `cache/usage.json` (`schemaVersion: 2`) under Koyo's store.
//!
//! One failed request never blanks an account: a failure updates the error and backoff fields
//! and never touches the last good reading (stale-on-error). The table is shared by every
//! collector, so each learns from the others' fetches. Only measurements (`lastGood`) and fetch
//! state are stored; sentinels ("token expired", ...) are derived fresh on every pass and laid
//! over the read model ([`UsageEntry::sentinel`]), so a stale one can't outlive its cause.
//!
//! Locking, never across network I/O: (a) lock, read, decide and claim the fetch set (a
//! bounded lease, `claimUntil`), unlock; (b) fetch with no lock; (c) lock, re-read, merge the
//! outcomes, clear the claims, write, unlock. The lease lets concurrent collectors skip
//! accounts another is fetching; recording releases it, and a crashed claimer's ages out.
//!
//! The store's I/O is in `store.rs`, the rules over one row in `rows.rs`, the backoff and
//! trust rules in `backoff.rs`. The numbers are cswap's but two, as `docs/claude-accounts.md`
//! 4 decides (Orca's cadence): [`BACKOFF_CAP_S`] is 900 s (cswap: 600), and the `Retry-After`
//! margin applies over [`RETRY_AFTER_MARGIN_ABOVE_S`] (600 s, cswap's measured threshold, which
//! cswap wrote as `> BACKOFF_CAP_S`), so raising the cap leaves that rule as cswap measured it.
//! The cache folder isn't narrowed to 0700 as cswap's `atomic_write_json` does; the file is
//! 0600.

mod backoff;
mod rows;
mod store;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde_json::Value;

use crate::poll_policy::{EXHAUSTED_INTERVAL_S, JITTER_FRAC, RECENT_429_WINDOW_S, RESET_SLACK_S};

pub use crate::poll_policy::SERVE_TTL_S;
pub use backoff::{
    AUTH_DEAD_STRIKES, BACKOFF_BASE_S, BACKOFF_CAP_S, BACKOFF_MAX_SHIFT, PERMANENT_AUTH_ERRORS,
    RATE_LIMIT_TRUST_MAX_AGE_S, RETRY_AFTER_FLOOR_CAP_S, RETRY_AFTER_MARGIN_ABOVE_S,
    RETRY_AFTER_MARGIN_S, TRUST_MAX_AGE_S, earliest_reset, failure_backoff_s,
    rate_limited_trust_ok, rejection_backoff_until, rejection_deadline,
};
pub use rows::row_eligible;
pub use store::UsageStore;

pub const SCHEMA_VERSION: i64 = 2;

/// Trusted for switch decisions up to this age; older is unknown headroom.
pub const STALE_OK_S: f64 = 300.;
/// The fetch lease: it covers a fetch, the inventory's stagger and queueing, so no other
/// surface reclaims a request still in flight; a crashed collector's 90 s stays under the
/// provider-safe interval.
pub const CLAIM_TTL_S: f64 = 90.;
/// The lease of rows written by a collector that predates `claimUntil`.
pub const LEGACY_CLAIM_TTL_S: f64 = 10.;

/// `(email, organizationUuid)`: the identity a slot number maps to now.
pub type Identity = (String, String);
/// Slot number → identity, the map every store call takes.
pub type Identities = BTreeMap<String, Identity>;
/// A slot's poll plan: `(nextPollAt, pollIntervalS)`.
pub type PollPlan = (Option<f64>, Option<f64>);

/// Whether a collector's fetch lease on a row is live: `claim_until` when there is one, else
/// (rows of an older collector) `last_attempt_at` plus [`LEGACY_CLAIM_TTL_S`]. The one test of
/// liveness, so `UsageEntry::claimed`, `entries` and `row_eligible` agree.
pub fn live_claim(claim_until: Option<f64>, last_attempt_at: Option<f64>, now: f64) -> bool {
    match claim_until {
        Some(until) => now < until,
        None => last_attempt_at.is_some_and(|at| now - at < LEGACY_CLAIM_TTL_S),
    }
}

/// The outcome of one fetch, as [`UsageStore::record`] takes it. One of three shapes: success
/// (`error` and `sentinel` `None`; `usage` may be `None`, an answer with no window), failure
/// (`error`, perhaps `retry_after_s`), or a sentinel, recorded as a no-op save for the refused
/// credential stamp it may carry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FetchRecord {
    pub usage: Option<Value>,
    pub error: Option<String>,
    pub retry_after_s: Option<f64>,
    pub sentinel: Option<String>,
    /// The fingerprint of the credential whose refresh token was sent when a permanent auth
    /// error came back: the strike binds to it, so any path that writes a new credential heals
    /// it. `None` binds unconditionally, as legacy writers did.
    pub struck_fp: Option<String>,
    /// The hash of the access token a live session's read was refused with: the one thing a
    /// sentinel record stores, so later passes skip the request while it's still the token.
    pub rejected_fp: Option<String>,
}

impl FetchRecord {
    pub fn success(usage: Option<Value>) -> FetchRecord {
        FetchRecord {
            usage,
            ..FetchRecord::default()
        }
    }

    pub fn failure(error: &str, retry_after_s: Option<f64>) -> FetchRecord {
        FetchRecord {
            error: Some(error.to_owned()),
            retry_after_s,
            ..FetchRecord::default()
        }
    }

    pub fn sentinel(sentinel: &str) -> FetchRecord {
        FetchRecord {
            sentinel: Some(sentinel.to_owned()),
            ..FetchRecord::default()
        }
    }
}

/// What a switch decision runs on: a sentinel or a trusted reading ([`UsageEntry::decision_value`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision<'a> {
    Sentinel(&'a str),
    Usage(&'a Value),
}

impl<'a> Decision<'a> {
    /// The reading, for `oauth::account_headroom` and the like; `None` for a sentinel.
    pub fn usage(self) -> Option<&'a Value> {
        match self {
            Decision::Usage(usage) => Some(usage),
            Decision::Sentinel(_) => None,
        }
    }
}

/// One account's usage state when read. `sentinel` is the collector's overlay (never stored);
/// the rest mirrors the stored row, but `age_s` (the last good reading's age) and
/// `trust_extended`, computed when read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageEntry {
    pub sentinel: Option<String>,
    pub last_good: Option<Value>,
    pub fetched_at: Option<f64>,
    pub age_s: Option<f64>,
    pub last_attempt_at: Option<f64>,
    pub consecutive_failures: i64,
    pub last_error: Option<String>,
    pub backoff_until: Option<f64>,
    pub next_poll_at: Option<f64>,
    pub poll_interval_s: Option<f64>,
    /// When this token last answered 429. Kept across later successes: the planner floors the
    /// cadence while a 429 is recent ([`UsageEntry::recent_429`]).
    pub last_429_at: Option<f64>,
    /// Consecutive permanent-auth failures; at [`AUTH_DEAD_STRIKES`] the token is dead.
    pub auth_dead_strikes: i64,
    /// The generation the strikes condemned (absent on legacy rows: they bind regardless).
    pub struck_fingerprint: Option<String>,
    /// The refused access token a live session's read stamped; cleared by a success.
    pub rejected_fingerprint: Option<String>,
    /// Staleness past [`STALE_OK_S`] still trusted because it is deliberate: the server refuses
    /// fresher data, the scheduler chose the cadence, a lease or a hold is live. Bounded.
    pub trust_extended: bool,
    pub claim_until: Option<f64>,
    /// Until when a reading adopted from another machine keeps every collector off the slot.
    pub held_until: Option<f64>,
}

impl UsageEntry {
    /// Fetched within [`SERVE_TTL_S`].
    pub fn fresh(&self, now: f64) -> bool {
        self.fresh_within(now, SERVE_TTL_S)
    }

    pub fn fresh_within(&self, now: f64, ttl: f64) -> bool {
        self.fetched_at.is_some_and(|at| now - at <= ttl)
    }

    pub fn in_backoff(&self, now: f64) -> bool {
        self.backoff_until.is_some_and(|until| now < until)
    }

    /// Whether an adopted reading's hold still keeps collectors off.
    pub fn held(&self, now: f64) -> bool {
        self.held_until.is_some_and(|until| now < until)
    }

    /// Whether this token 429'd recently enough to keep the post-429 cadence.
    ///
    /// Recency runs from when the 429's backoff lifts, not from the 429: an hour-scale
    /// `Retry-After` is one long backoff with no attempt in it, so measured from the stamp the
    /// first success after it would see the window already gone, and neither the AIMD growth
    /// nor the floor would engage. The anchor moves only while the live backoff is a 429's:
    /// `last_429_at` is never cleared, and an unrelated later timeout must not re-arm it.
    pub fn recent_429(&self, now: f64) -> bool {
        let Some(mut anchor) = self.last_429_at else {
            return false;
        };
        if self.last_error.as_deref() == Some("http-429")
            && let Some(until) = self.backoff_until.filter(|until| *until > anchor)
        {
            anchor = until;
        }
        now < anchor + RECENT_429_WINDOW_S
    }

    /// Whether another collector's fetch lease is live.
    pub fn claimed(&self, now: f64) -> bool {
        live_claim(self.claim_until, self.last_attempt_at, now)
    }

    /// Whether the stored credential's refresh-token lineage is provably dead: struck
    /// [`AUTH_DEAD_STRIKES`] times without a success. Such an account is quarantined, not
    /// fetched, and shows "re-login needed". Strikes condemn the generation sent, not the slot:
    /// given the stored credential's fingerprint (`stored_fp`) and a different struck one, the
    /// credential was replaced since and the strike no longer applies.
    pub fn token_dead(&self, stored_fp: Option<&str>) -> bool {
        if self.auth_dead_strikes < AUTH_DEAD_STRIKES {
            return false;
        }
        !matches!(
            (stored_fp, self.struck_fingerprint.as_deref()),
            (Some(stored), Some(struck)) if stored != struck
        )
    }

    /// What switch decisions run on: the sentinel when set; else the last good reading while
    /// it is recent enough ([`STALE_OK_S`], or `trust_extended`); else `None`, unknown. Views
    /// read `last_good` and `age_s` directly and may show older data with its age.
    pub fn decision_value(&self) -> Option<Decision<'_>> {
        if let Some(sentinel) = &self.sentinel {
            return Some(Decision::Sentinel(sentinel));
        }
        let trusted = self
            .age_s
            .is_some_and(|age| age <= STALE_OK_S || self.trust_extended);
        self.last_good
            .as_ref()
            .filter(|_| trusted)
            .map(Decision::Usage)
    }
}

/// Whether a deadline can't have come from the bounded planner.
pub(crate) fn plan_oversleeps(
    next_poll_at: Option<f64>,
    poll_interval_s: Option<f64>,
    now: f64,
) -> bool {
    let Some(next_poll_at) = next_poll_at else {
        return false;
    };
    let interval = poll_interval_s
        .filter(|v| *v != 0.)
        .unwrap_or(EXHAUSTED_INTERVAL_S)
        .max(EXHAUSTED_INTERVAL_S);
    next_poll_at > now + interval * (1. + JITTER_FRAC) + RESET_SLACK_S
}

/// Whether a row carries an obsolete reset-parked plan: a distant reset deadline kept with a
/// much shorter learned interval. Told by its shape, whatever models are selected now, so
/// changing them can't leave an account parked until the old reset.
pub fn plan_oversleeps_interval(entry: &UsageEntry, now: f64) -> bool {
    plan_oversleeps(entry.next_poll_at, entry.poll_interval_s, now)
}

/// The due candidate least recently attempted or measured, or `None`: the auto engine's one
/// alternate per tick.
///
/// Due is past its `nextPollAt`, not in backoff, not held for another machine's reading, not a
/// sentinel and not quarantined; a failing account's backoff takes it out between attempts. The
/// least recently touched wins: the later of the last attempt (`reserve` stamps it) and the last
/// measurement, so a fetch-path sentinel, which records no measurement, rotates to the back
/// instead of taking every tick; a row never touched keys 0.
pub fn due_candidate(
    candidates: &[&str],
    entries: &BTreeMap<String, UsageEntry>,
    now: f64,
) -> Option<String> {
    let mut due: Vec<(f64, &str)> = Vec::new();
    for &num in candidates {
        let Some(entry) = entries.get(num) else {
            due.push((0., num));
            continue;
        };
        let parked =
            entry.next_poll_at.is_some_and(|at| now < at) && !plan_oversleeps_interval(entry, now);
        if entry.sentinel.is_some()
            || entry.token_dead(None)
            || entry.in_backoff(now)
            || entry.held(now)
            || parked
        {
            continue;
        }
        let touched = entry
            .fetched_at
            .unwrap_or(0.)
            .max(entry.last_attempt_at.unwrap_or(0.));
        due.push((touched, num));
    }
    due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
    due.first().map(|(_, num)| (*num).to_owned())
}

/// Lays a derived sentinel over a stored entry (read model only).
pub fn with_sentinel(entry: UsageEntry, sentinel: Option<&str>) -> UsageEntry {
    match sentinel {
        Some(sentinel) => UsageEntry {
            sentinel: Some(sentinel.to_owned()),
            ..entry
        },
        None => entry,
    }
}
