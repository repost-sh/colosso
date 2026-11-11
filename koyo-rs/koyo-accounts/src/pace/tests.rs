//! Tests translated from claude-swap's `tests/test_pace.py` (0fe22a8), named after cswap's.
//! `TestProjectedExhaustionTs`, `TestWillLastToReset` and `TestAheadVsWillLastRelationship` are
//! left out with the script-only functions they test.

#![expect(
    clippy::float_cmp,
    reason = "cswap's assertions are exact: whole days of seconds, and percentages of them"
)]

use serde_json::{Value, json};

use super::*;
use crate::testing::iso;

const NOW: f64 = 1_700_000_000.;
const DAY: f64 = 86_400.;
const WEEK: f64 = WEEKLY_PERIOD_S;

fn window(pct: f64, resets_at_ts: Option<f64>) -> Value {
    let mut window = json!({"pct": pct});
    if let Some(at) = resets_at_ts {
        window["resets_at"] = json!(iso(at));
    }
    window
}

fn pace(pct: f64, resets_at_ts: f64) -> Option<PaceResult> {
    compute_pace(Some(&window(pct, Some(resets_at_ts))), Some(NOW))
}

// TestComputePaceElapsed

#[test]
fn one_day_into_the_week() {
    let result = pace(20., NOW + 6. * DAY).unwrap();
    assert_eq!(result.elapsed_s, DAY);
    assert_eq!(result.expected_pct, DAY / WEEK * 100.);
}

#[test]
fn right_at_reset_boundary_is_suppressed() {
    assert_eq!(pace(5., NOW + WEEK), None);
}

#[test]
fn stale_resets_at_multiple_cycles_in_the_past_still_resolves() {
    let result = pace(20., NOW - 2. * WEEK - DAY).unwrap();
    assert_eq!(result.elapsed_s, DAY);
}

#[test]
fn missing_fields_return_none() {
    assert_eq!(compute_pace(None, Some(NOW)), None);
    assert_eq!(compute_pace(Some(&json!({"pct": 10.0})), Some(NOW)), None);
    assert_eq!(
        compute_pace(Some(&json!({"resets_at": iso(NOW + DAY)})), Some(NOW)),
        None
    );
    assert_eq!(
        compute_pace(Some(&window(10., Some(NOW + DAY))), None),
        None
    );
    assert_eq!(
        compute_pace(
            Some(&json!({"pct": 10.0, "resets_at": "not-a-date"})),
            Some(NOW)
        ),
        None
    );
}

// TestSuppressionWindow

#[test]
fn just_inside_suppression_window_is_none() {
    let elapsed = SUPPRESS_AFTER_RESET_S - 1.;
    assert_eq!(pace(50., NOW + WEEK - elapsed), None);
}

#[test]
fn just_outside_suppression_window_is_not_none() {
    let elapsed = SUPPRESS_AFTER_RESET_S;
    let result = pace(50., NOW + WEEK - elapsed).unwrap();
    assert_eq!(result.elapsed_s, elapsed);
}

// TestAheadThreshold

#[test]
fn meaningfully_ahead_flags_true() {
    assert!(pace(50., NOW + 6. * DAY).unwrap().ahead);
}

#[test]
fn within_threshold_flags_false_but_still_returns() {
    assert!(!pace(20., NOW + 6. * DAY).unwrap().ahead);
}

#[test]
fn behind_pace_flags_false() {
    assert!(!pace(5., NOW + 6. * DAY).unwrap().ahead);
}
