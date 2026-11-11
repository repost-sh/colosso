//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestLast429Marker`, `TestRecent429AcrossHonoredBlock`,
//! `TestHourScale429FloorEngagesThroughStore`.

use std::collections::BTreeMap;

use super::*;
use crate::poll_policy::{
    POST_429_MAX_INTERVAL_S, POST_429_MIN_INTERVAL_S, PlanInput, RECENT_429_WINDOW_S,
    plan_after_fetch,
};

// TestLast429Marker

#[test]
fn last_429_survives_recovery() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 0.));
    let t429 = h.now();
    h.advance(400.);
    h.record("1", ok());
    let entry = h.entry("1");
    assert_eq!(entry.consecutive_failures, 0);
    assert_eq!(entry.last_429_at, Some(t429));
}

#[test]
fn non_429_failures_leave_the_marker_alone() {
    let h = Harness::new();
    h.record("1", err("timeout"));
    assert_eq!(h.entry("1").last_429_at, None);
}

// TestRecent429AcrossHonoredBlock

#[test]
fn recent_429_true_at_first_success_after_hour_block() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 3600.));
    let before = h.entry("1");
    h.set(before.backoff_until.unwrap());
    assert!(before.recent_429(h.now()));
}

#[test]
fn recent_429_false_once_window_truly_elapsed() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 3600.));
    let before = h.entry("1");
    h.set(before.backoff_until.unwrap() + RECENT_429_WINDOW_S + 1.);
    assert!(!before.recent_429(h.now()));
}

#[test]
fn short_retry_after_recency_still_expires_normally() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 0.));
    let before = h.entry("1");
    h.set(before.backoff_until.unwrap());
    assert!(before.recent_429(h.now()));
    h.advance(RECENT_429_WINDOW_S);
    assert!(!before.recent_429(h.now()));
}

#[test]
fn unrelated_timeout_does_not_re_arm_recency() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 0.));
    h.advance(400.);
    h.record("1", ok());
    h.advance(RECENT_429_WINDOW_S + 5000.);
    assert!(!h.entry("1").recent_429(h.now()));
    h.record("1", err("timeout"));
    let before = h.entry("1");
    assert_eq!(before.last_error.as_deref(), Some("timeout"));
    assert!(before.last_429_at.is_some());
    h.set(before.backoff_until.unwrap());
    assert!(!before.recent_429(h.now()));
}

// TestHourScale429FloorEngagesThroughStore

fn plan(h: &Harness, before: &UsageEntry, after: &UsageEntry, recent_429: bool) -> (f64, f64) {
    plan_after_fetch(&PlanInput {
        prev_interval_s: before.poll_interval_s,
        prev_usage: before.last_good.as_ref(),
        new_usage: after.last_good.as_ref(),
        is_active: false,
        threshold: 90.,
        models: &[],
        recent_429,
        now: h.now(),
        weekly_threshold: None,
        rng: 0.5,
    })
}

fn plan_after_first_success(legacy_recency: bool) -> (bool, f64) {
    let h = Harness::new();
    h.record("1", err_after("http-429", 3600.));
    let before = h.entry("1");
    h.set(before.backoff_until.unwrap());
    h.record("1", ok());
    let after = h.entry("1");
    let recent = if legacy_recency {
        before
            .last_429_at
            .is_some_and(|at| h.now() - at < RECENT_429_WINDOW_S)
    } else {
        before.recent_429(h.now())
    };
    (recent, plan(&h, &before, &after, recent).1)
}

#[test]
fn floor_engages_at_first_post_block_success() {
    let (recent, interval) = plan_after_first_success(false);
    assert!(recent);
    assert!(interval >= POST_429_MIN_INTERVAL_S);
}

#[test]
fn legacy_recency_would_drop_the_floor() {
    let (recent, interval) = plan_after_first_success(true);
    assert!(!recent);
    assert!(interval < POST_429_MIN_INTERVAL_S);
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the interval saturates at the ceiling itself, not near it"
)]
fn repeated_429_episodes_converge_to_the_wide_ceiling() {
    let h = Harness::new();
    let mut intervals = Vec::new();
    for _ in 0..6 {
        h.record("1", err_after("http-429", 60.));
        let before = h.entry("1");
        h.set(before.backoff_until.unwrap());
        h.record("1", ok());
        let after = h.entry("1");
        let (next, interval) = plan(&h, &before, &after, before.recent_429(h.now()));
        h.store
            .set_poll_plan(
                &BTreeMap::from([("1".to_owned(), (Some(next), Some(interval)))]),
                &ident(),
            )
            .unwrap();
        intervals.push(interval);
        h.advance(10.);
    }
    assert!(intervals.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(*intervals.last().unwrap(), POST_429_MAX_INTERVAL_S);
    assert!(intervals[0] < intervals[2] && intervals[2] < POST_429_MAX_INTERVAL_S);
}
