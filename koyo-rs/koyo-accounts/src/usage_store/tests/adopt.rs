//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestAdopt`: readings another machine took for the same account.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::*;

fn adopt(h: &Harness, readings: Vec<(&str, Value, f64)>, hold_s: Option<f64>) -> BTreeSet<String> {
    let readings: BTreeMap<String, (Value, f64)> = readings
        .into_iter()
        .map(|(num, usage, age)| (num.to_owned(), (usage, age)))
        .collect();
    h.store.adopt(&readings, &ident(), hold_s).unwrap()
}

#[test]
fn reading_is_backdated_by_its_age() {
    let h = Harness::new();
    assert_eq!(adopt(&h, vec![("1", usage(), 40.)], None), set(&["1"]));
    let entry = h.entry("1");
    assert_eq!(entry.last_good, Some(usage()));
    assert_eq!(entry.fetched_at, Some(h.now() - 40.));
    assert_eq!(entry.age_s, Some(40.));
}

#[test]
fn never_downgrades_a_newer_local_reading() {
    let h = Harness::new();
    let local = json!({"five_hour": {"pct": 60.0}});
    h.record("1", FetchRecord::success(Some(local.clone())));
    h.advance(10.);
    assert!(adopt(&h, vec![("1", usage(), 30.)], None).is_empty());
    assert_eq!(h.entry("1").last_good, Some(local));
}

#[test]
fn fetch_state_is_left_alone() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 0.));
    let before = h.entry("1");
    adopt(&h, vec![("1", usage(), 0.)], None);
    let after = h.entry("1");
    assert_eq!(after.last_good, Some(usage()));
    assert_eq!(after.consecutive_failures, 1);
    assert_eq!(before.consecutive_failures, 1);
    assert_eq!(after.last_error.as_deref(), Some("http-429"));
    assert_eq!(after.backoff_until, before.backoff_until);
    assert_eq!(after.last_429_at, before.last_429_at);
}

#[test]
fn hold_keeps_every_mode_off_until_it_lapses() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.)], Some(600.));
    h.advance(SERVE_TTL_S + 1.);
    assert!(h.reserve(&["1"], true).is_empty());
    assert!(h.reserve(&["1"], false).is_empty());
    assert!(h.reserve_repair(&["1"], false).is_empty());
    h.advance(600.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn held_reading_stays_decision_trusted() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.)], Some(900.));
    h.advance(STALE_OK_S + 1.);
    let entry = h.entry("1");
    assert!(entry.held(h.now()));
    assert_eq!(decision(&entry), Some(&usage()));
    h.advance(900.);
    assert_eq!(h.entry("1").decision_value(), None);
}

#[test]
fn hold_never_outlives_the_trust_ceiling() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 3000.)], Some(3600.));
    let entry = h.entry("1");
    assert!(approx(
        entry.held_until.unwrap(),
        entry.fetched_at.unwrap() + TRUST_MAX_AGE_S
    ));
    h.advance(TRUST_MAX_AGE_S - 3000.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn latest_hold_wins() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.)], Some(900.));
    adopt(&h, vec![("1", usage(), 0.)], Some(60.));
    assert_eq!(h.entry("1").held_until, Some(h.now() + 60.));
}

/// A refused probe's reading, its `resets_at` `deadline_in` from now: with a Retry-After the
/// server's deadline, without one the quota reset.
fn rejected_reading(now: f64, deadline_in: f64, retry_after_s: Option<f64>) -> Value {
    let mut reading = usage();
    reading["rejected"] = json!({
        "retry_after_s": retry_after_s,
        "resets_at": crate::oauth::utc_at(now + deadline_in).unwrap().to_rfc3339(),
    });
    reading["source"] = json!("headers");
    reading
}

#[test]
fn rejection_deadline_carries_over_as_backoff() {
    let h = Harness::new();
    let reading = rejected_reading(h.now(), 1800., Some(3600.));
    assert_eq!(adopt(&h, vec![("1", reading, 1800.)], None), set(&["1"]));
    let entry = h.entry("1");
    assert!(approx(entry.backoff_until.unwrap(), h.now() + 1800.));
    assert!(entry.last_error.is_none() && entry.consecutive_failures == 0);
    h.advance(SERVE_TTL_S + 1.);
    assert!(h.reserve(&["1"], false).is_empty());
    assert!(h.reserve(&["1"], true).is_empty());
    h.advance(1800.);
    assert_eq!(won(&h.reserve(&["1"], false)), ["1"]);
}

#[test]
fn rejection_without_retry_after_installs_no_backoff() {
    let h = Harness::new();
    let reading = rejected_reading(h.now(), 1800., None);
    assert_eq!(
        adopt(&h, vec![("1", reading.clone(), 0.)], None),
        set(&["1"])
    );
    assert_eq!(h.entry("1").backoff_until, None);
    h.record("2", FetchRecord::success(Some(reading)));
    assert_eq!(h.entry("2").backoff_until, None);
}

#[test]
fn past_or_unadopted_rejection_leaves_backoff_alone() {
    let h = Harness::new();
    adopt(
        &h,
        vec![("1", rejected_reading(h.now(), -10., Some(3600.)), 0.)],
        None,
    );
    assert_eq!(h.entry("1").backoff_until, None);
    h.record("2", ok());
    h.advance(10.);
    let late = rejected_reading(h.now(), 1800., Some(3600.));
    assert!(adopt(&h, vec![("2", late, 30.)], None).is_empty());
    assert_eq!(h.entry("2").backoff_until, None);
}

#[test]
fn rejection_deadline_never_shortens_and_is_capped() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 4000.));
    let parked = h.entry("1").backoff_until.unwrap();
    assert!(parked > h.now() + 1800.);
    adopt(
        &h,
        vec![("1", rejected_reading(h.now(), 1800., Some(3600.)), 0.)],
        None,
    );
    assert_eq!(h.entry("1").backoff_until, Some(parked));
    adopt(
        &h,
        vec![(
            "2",
            rejected_reading(h.now(), 10. * 86_400., Some(3600.)),
            0.,
        )],
        None,
    );
    assert!(approx(
        h.entry("2").backoff_until.unwrap(),
        h.now() + RETRY_AFTER_FLOOR_CAP_S
    ));
}

fn usage_resetting_in(now: f64, five_hour_s: f64, scoped_s: Option<f64>) -> Value {
    let mut reading = json!({
        "five_hour": {"pct": 25.0, "resets_at": iso_in(now, five_hour_s)},
        "seven_day": {"pct": 10.0, "resets_at": iso_in(now, 86_400.)},
    });
    if let Some(ahead) = scoped_s {
        reading["scoped"] = json!([{"name": "Fable", "pct": 5.0, "resets_at": iso_in(now, ahead)}]);
    }
    reading
}

#[test]
fn hold_stops_at_the_readings_earliest_reset() {
    let h = Harness::new();
    adopt(
        &h,
        vec![("1", usage_resetting_in(h.now(), 300., None), 0.)],
        Some(900.),
    );
    assert!(approx(h.entry("1").held_until.unwrap(), h.now() + 300.));
    h.advance(300.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn a_scoped_windows_reset_caps_the_hold_too() {
    let h = Harness::new();
    let reading = usage_resetting_in(h.now(), 3000., Some(120.));
    adopt(&h, vec![("1", reading, 0.)], Some(900.));
    assert!(approx(h.entry("1").held_until.unwrap(), h.now() + 120.));
}

#[test]
fn the_cap_comes_from_the_reading_kept() {
    let h = Harness::new();
    h.record(
        "1",
        FetchRecord::success(Some(usage_resetting_in(h.now(), 120., None))),
    );
    let imported = usage_resetting_in(h.now(), 3000., None);
    assert!(adopt(&h, vec![("1", imported, 30.)], Some(900.)).is_empty());
    assert!(approx(h.entry("1").held_until.unwrap(), h.now() + 120.));
}

#[test]
fn a_reading_from_before_a_reset_is_not_held() {
    let h = Harness::new();
    let reading = usage_resetting_in(h.now(), -60., None);
    assert_eq!(
        adopt(&h, vec![("1", reading, 600.)], Some(900.)),
        set(&["1"])
    );
    assert_eq!(h.entry("1").held_until, None);
}

#[test]
fn zero_hold_lifts_an_existing_hold() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.)], Some(900.));
    adopt(&h, vec![("1", usage(), 0.)], Some(0.));
    assert_eq!(h.entry("1").held_until, None);
    h.advance(SERVE_TTL_S + 1.);
    assert_eq!(won(&h.reserve(&["1"], true)), ["1"]);
}

#[test]
fn no_hold_leaves_an_existing_hold_alone() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.)], Some(900.));
    adopt(&h, vec![("1", usage(), 0.)], None);
    assert_eq!(h.entry("1").held_until, Some(h.now() + 900.));
}

#[test]
fn due_candidate_skips_a_held_slot() {
    let h = Harness::new();
    adopt(&h, vec![("1", usage(), 0.), ("2", usage(), 0.)], Some(600.));
    let plan = (Some(h.now() + 60.), Some(60.));
    let plans = BTreeMap::from([("1".to_owned(), plan), ("2".to_owned(), plan)]);
    h.store.set_poll_plan(&plans, &ident()).unwrap();
    h.advance(61.);
    let table = h.store.entries(&ident(), &[]);
    assert_eq!(due_candidate(&["1", "2"], &table, h.now()), None);
}

#[test]
fn a_claimed_fetch_still_records() {
    let h = Harness::new();
    let claims = h.reserve(&["1"], true);
    adopt(&h, vec![("1", usage(), 5.)], Some(600.));
    let local = json!({"five_hour": {"pct": 61.0}});
    let accepted = h.record_fenced("1", FetchRecord::success(Some(local.clone())), &claims);
    assert_eq!(accepted, set(&["1"]));
    assert_eq!(h.entry("1").last_good, Some(local));
}

#[test]
fn row_of_another_account_is_replaced() {
    let h = Harness::new();
    h.record_as("2", ok(), &identity("2", "old@x.com", ""));
    adopt(&h, vec![("2", usage(), 0.)], None);
    let raw = h.raw();
    let row = &raw["accounts"]["2"];
    assert_eq!(row["email"], "b@x.com");
    assert_eq!(row["organizationUuid"], "org-2");
}
