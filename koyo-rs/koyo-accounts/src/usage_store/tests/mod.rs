//! Tests translated from claude-swap's `tests/test_usage_store.py` (0fe22a8), named after
//! cswap's, on a sandbox store whose clock starts where cswap's `FakeClock` does. Where cswap
//! asserts its 600 s backoff cap, the spec's 900 s is asserted instead, and said so.

mod adopt;
mod backoff;
mod claims;
mod due;
mod recency;
mod schema_trust;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::*;
use crate::testing::Sandbox;

/// cswap's `FakeClock` start.
pub(super) const START: f64 = 1_000_000.;

/// cswap's `IDENT`.
pub(super) fn ident() -> Identities {
    BTreeMap::from([
        ("1".to_owned(), ("a@x.com".to_owned(), String::new())),
        ("2".to_owned(), ("b@x.com".to_owned(), "org-2".to_owned())),
    ])
}

pub(super) fn identity(num: &str, email: &str, org: &str) -> Identities {
    BTreeMap::from([(num.to_owned(), (email.to_owned(), org.to_owned()))])
}

/// cswap's `USAGE`.
pub(super) fn usage() -> Value {
    json!({"five_hour": {"pct": 25.0}, "seven_day": {"pct": 10.0}})
}

pub(super) fn ok() -> FetchRecord {
    FetchRecord::success(Some(usage()))
}

pub(super) fn err(kind: &str) -> FetchRecord {
    FetchRecord::failure(kind, None)
}

pub(super) fn err_after(kind: &str, retry_after_s: f64) -> FetchRecord {
    FetchRecord::failure(kind, Some(retry_after_s))
}

/// The ISO stamp `ahead` seconds from `now`, as cswap's tests write them (`Z`).
pub(super) fn iso_in(now: f64, ahead: f64) -> String {
    crate::oauth::utc_at(now + ahead)
        .unwrap()
        .to_rfc3339()
        .replace("+00:00", "Z")
}

pub(super) fn approx(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6 * right.abs().max(1.)
}

/// A sandbox and its usage table.
pub(super) struct Harness {
    pub sandbox: Sandbox,
    pub store: UsageStore,
}

impl Harness {
    pub fn new() -> Harness {
        let sandbox = Sandbox::new();
        sandbox.clock.set(START);
        let store = UsageStore::new(&sandbox.env());
        Harness { sandbox, store }
    }

    pub fn now(&self) -> f64 {
        self.sandbox.env().now()
    }

    pub fn advance(&self, seconds: f64) {
        self.sandbox.clock.advance(seconds);
    }

    pub fn set(&self, now: f64) {
        self.sandbox.clock.set(now);
    }

    /// `store.record({num: record}, identities)`, unfenced.
    pub fn record_as(
        &self,
        num: &str,
        record: FetchRecord,
        identities: &Identities,
    ) -> BTreeSet<String> {
        self.store
            .record(
                &BTreeMap::from([(num.to_owned(), record)]),
                identities,
                None,
                None,
            )
            .unwrap()
    }

    /// `store.record({num: record}, IDENT)`.
    pub fn record(&self, num: &str, record: FetchRecord) -> BTreeSet<String> {
        self.record_as(num, record, &ident())
    }

    /// `store.record({num: record}, IDENT, claims)`.
    pub fn record_fenced(
        &self,
        num: &str,
        record: FetchRecord,
        claims: &BTreeMap<String, String>,
    ) -> BTreeSet<String> {
        self.store
            .record(
                &BTreeMap::from([(num.to_owned(), record)]),
                &ident(),
                Some(claims),
                None,
            )
            .unwrap()
    }

    pub fn entry_as(&self, num: &str, identities: &Identities) -> UsageEntry {
        self.store.entries(identities, &[]).remove(num).unwrap()
    }

    /// `store.entries(IDENT)[num]`.
    pub fn entry(&self, num: &str) -> UsageEntry {
        self.entry_as(num, &ident())
    }

    pub fn reserve(&self, nums: &[&str], respect_plans: bool) -> BTreeMap<String, String> {
        self.store
            .reserve(nums, &ident(), respect_plans, false)
            .unwrap()
    }

    pub fn reserve_repair(&self, nums: &[&str], respect_plans: bool) -> BTreeMap<String, String> {
        self.store
            .reserve(nums, &ident(), respect_plans, true)
            .unwrap()
    }

    pub fn plan(&self, num: &str, next_poll_at: Option<f64>, interval: Option<f64>) {
        self.store
            .set_poll_plan(
                &BTreeMap::from([(num.to_owned(), (next_poll_at, interval))]),
                &ident(),
            )
            .unwrap();
    }

    pub fn raw(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(&self.store.path).unwrap()).unwrap()
    }

    pub fn write_raw(&self, text: &str) {
        std::fs::create_dir_all(self.store.path.parent().unwrap()).unwrap();
        std::fs::write(&self.store.path, text).unwrap();
    }
}

pub(super) fn won(claims: &BTreeMap<String, String>) -> Vec<&str> {
    claims.keys().map(String::as_str).collect()
}

pub(super) fn set(nums: &[&str]) -> BTreeSet<String> {
    nums.iter().map(|num| (*num).to_owned()).collect()
}

pub(super) fn decision(entry: &UsageEntry) -> Option<&Value> {
    match entry.decision_value() {
        Some(Decision::Usage(usage)) => Some(usage),
        _ => None,
    }
}
