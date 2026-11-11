//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestSchema`, `TestStaleOnError`, `TestExtendedTrust`, `TestRateLimitTrustBounds`.

use serde_json::{Value, json};

use super::*;

// TestSchema

#[test]
fn empty_when_missing() {
    let h = Harness::new();
    let entry = h.entry("1");
    assert_eq!(entry, UsageEntry::default());
    assert_eq!(entry.decision_value(), None);
}

#[test]
fn versionless_legacy_snapshot_ignored() {
    let h = Harness::new();
    h.write_raw(&json!({"timestamp": 123, "data": {"1": usage()}}).to_string());
    assert_eq!(h.entry("1").last_good, None);
}

#[test]
fn corrupt_file_ignored() {
    let h = Harness::new();
    h.write_raw("{not json");
    assert_eq!(h.entry("1"), UsageEntry::default());
}

#[test]
fn round_trip() {
    let h = Harness::new();
    h.record("1", ok());
    let raw = h.raw();
    assert_eq!(raw["schemaVersion"], 2);
    let row = &raw["accounts"]["1"];
    assert_eq!(row["email"], "a@x.com");
    assert_eq!(row["lastGood"], usage());
    assert_eq!(row["fetchedAt"].as_f64(), Some(h.now()));
    let entry = h.entry("1");
    assert_eq!(entry.last_good, Some(usage()));
    assert_eq!(entry.age_s, Some(0.));
    assert_eq!(decision(&entry), Some(&usage()));
}

#[test]
fn the_table_lives_in_the_stores_cache() {
    let h = Harness::new();
    h.record("1", ok());
    let cache = crate::paths::store::cache_dir(&h.sandbox.env());
    assert_eq!(h.store.path, cache.join("usage.json"));
    assert!(cache.join("usage.json").is_file());
    assert!(cache.join(".usage.lock").exists());
}

// TestStaleOnError

#[test]
fn failure_preserves_last_good() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(60.);
    h.record("1", err("http-429"));
    let entry = h.entry("1");
    assert_eq!(entry.last_good, Some(usage()));
    assert_eq!(entry.age_s, Some(60.));
    assert_eq!(entry.last_error.as_deref(), Some("http-429"));
    assert_eq!(entry.consecutive_failures, 1);
    // Still trusted for decisions within STALE_OK_S.
    assert_eq!(decision(&entry), Some(&usage()));
}

#[test]
fn too_stale_is_unknown_for_decisions() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(STALE_OK_S + 1.);
    let entry = h.entry("1");
    assert_eq!(entry.decision_value(), None);
    // ... but a view still sees the measurement and its age.
    assert_eq!(entry.last_good, Some(usage()));
    assert_eq!(entry.age_s, Some(STALE_OK_S + 1.));
}

#[test]
fn success_clears_failure_state() {
    let h = Harness::new();
    h.record("1", err("timeout"));
    h.advance(5.);
    h.record("1", ok());
    let entry = h.entry("1");
    assert_eq!(entry.consecutive_failures, 0);
    assert_eq!(entry.last_error, None);
    assert_eq!(entry.backoff_until, None);
    assert_eq!(decision(&entry), Some(&usage()));
}

#[test]
fn success_with_no_windows() {
    let h = Harness::new();
    h.record("1", FetchRecord::success(None));
    let entry = h.entry("1");
    assert_eq!(entry.last_error, None);
    assert!(entry.fetched_at.is_some());
    assert_eq!(entry.decision_value(), None);
}

// TestExtendedTrust: deliberate staleness stays trusted.

#[test]
fn in_backoff_past_stale_ok_is_still_trusted() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(STALE_OK_S);
    h.record("1", err_after("http-429", 480.));
    h.advance(60.);
    let entry = h.entry("1");
    assert!(entry.age_s.unwrap() > STALE_OK_S);
    assert!(entry.in_backoff(h.now()));
    assert!(entry.trust_extended);
    assert_eq!(decision(&entry), Some(&usage()));
}

#[test]
fn failure_state_after_backoff_expiry_is_still_trusted() {
    let h = Harness::new();
    h.record("1", ok());
    h.advance(60.);
    h.record("1", err("timeout"));
    h.advance(BACKOFF_BASE_S + STALE_OK_S);
    let entry = h.entry("1");
    assert!(!entry.in_backoff(h.now()));
    assert_eq!(decision(&entry), Some(&usage()));
}

#[test]
fn within_poll_plan_past_stale_ok_is_trusted() {
    let h = Harness::new();
    h.record("1", ok());
    h.plan("1", Some(h.now() + 600.), Some(600.));
    h.advance(400.);
    let entry = h.entry("1");
    assert_eq!(entry.consecutive_failures, 0);
    assert_eq!(decision(&entry), Some(&usage()));
    // Once overdue, the staleness is no longer the scheduler's choice.
    h.advance(250.);
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn trust_ceiling_wins_over_non_429_failure_state() {
    let h = Harness::new();
    h.record("1", ok());
    h.record("1", err("timeout"));
    h.advance(TRUST_MAX_AGE_S + 1.);
    h.record("1", err("timeout"));
    let entry = h.entry("1");
    assert_eq!(entry.consecutive_failures, 2);
    assert_eq!(entry.decision_value(), None);
}

fn usage_resetting_at(now: f64, ahead: f64) -> Value {
    let at = iso_in(now, ahead);
    json!({
        "five_hour": {"pct": 25.0, "resets_at": at},
        "seven_day": {"pct": 10.0, "resets_at": at},
    })
}

#[test]
fn _429_staleness_trusted_until_window_reset() {
    let h = Harness::new();
    let reading = usage_resetting_at(
        h.now(),
        f64::midpoint(TRUST_MAX_AGE_S, RATE_LIMIT_TRUST_MAX_AGE_S),
    );
    h.record("1", FetchRecord::success(Some(reading.clone())));
    h.record("1", err("http-429"));
    h.advance(TRUST_MAX_AGE_S + 1.);
    h.record("1", err("http-429"));
    let entry = h.entry("1");
    assert!(entry.trust_extended);
    assert_eq!(decision(&entry), Some(&reading));
}

#[test]
fn _429_staleness_expires_at_window_reset() {
    let h = Harness::new();
    let reading = usage_resetting_at(h.now(), 600.);
    h.record("1", FetchRecord::success(Some(reading.clone())));
    h.record("1", err("http-429"));
    h.advance(601.);
    h.record("1", err("http-429"));
    let entry = h.entry("1");
    assert_eq!(entry.decision_value(), None);
    assert_eq!(entry.last_good, Some(reading));
}

#[test]
fn _429_staleness_without_reset_info_falls_back_to_ceiling() {
    let h = Harness::new();
    h.record("1", ok());
    h.record("1", err("http-429"));
    h.advance(TRUST_MAX_AGE_S + 1.);
    h.record("1", err("http-429"));
    assert_eq!(decision(&h.entry("1")), Some(&usage()));
    h.advance(RATE_LIMIT_TRUST_MAX_AGE_S);
    h.record("1", err("http-429"));
    assert_eq!(h.entry("1").decision_value(), None);
}

// TestRateLimitTrustBounds

fn bounded_usage(now: f64, five_h_ahead: Option<f64>, seven_d_ahead: Option<f64>) -> Value {
    let mut five = json!({"pct": 25.0});
    if let Some(ahead) = five_h_ahead {
        five["resets_at"] = json!(iso_in(now, ahead));
    }
    let mut seven = json!({"pct": 10.0});
    if let Some(ahead) = seven_d_ahead {
        seven["resets_at"] = json!(iso_in(now, ahead));
    }
    json!({"five_hour": five, "seven_day": seven})
}

fn after_429s(h: &Harness, reading: &Value, wait: f64) {
    h.record("1", FetchRecord::success(Some(reading.clone())));
    h.record("1", err("http-429"));
    h.advance(wait);
    h.record("1", err("http-429"));
}

#[test]
fn far_future_reset_is_clamped_to_the_ceiling() {
    let h = Harness::new();
    let far = 10. * 365. * 24. * 3600.;
    after_429s(
        &h,
        &bounded_usage(h.now(), Some(far), Some(far)),
        RATE_LIMIT_TRUST_MAX_AGE_S + 1.,
    );
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn trust_keys_on_earliest_future_reset() {
    let h = Harness::new();
    after_429s(
        &h,
        &bounded_usage(h.now(), Some(600.), Some(100. * 3600.)),
        601.,
    );
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn partial_metadata_still_bounded_by_ceiling() {
    let h = Harness::new();
    let reading = bounded_usage(h.now(), None, Some(100. * 3600.));
    after_429s(&h, &reading, RATE_LIMIT_TRUST_MAX_AGE_S + 1.);
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn ceiling_wins_when_reset_is_beyond_it() {
    let h = Harness::new();
    let far = RATE_LIMIT_TRUST_MAX_AGE_S * 3.;
    let reading = bounded_usage(h.now(), Some(far), Some(far));
    after_429s(&h, &reading, RATE_LIMIT_TRUST_MAX_AGE_S - 60.);
    assert_eq!(decision(&h.entry("1")), Some(&reading));
    h.advance(120.);
    h.record("1", err("http-429"));
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn earliest_reset_is_the_soonest_relevant_window() {
    let reading = bounded_usage(START, Some(600.), Some(100. * 3600.));
    assert_eq!(earliest_reset(Some(&reading), &[]), Some(START + 600.));
    assert_eq!(earliest_reset(Some(&Value::from("api key")), &[]), None);
}
