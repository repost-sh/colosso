//! cswap's `TestModelAwareSwitch` as far as it goes without the `autoswitch.model` setting (a
//! per-model window never binds: its cases with the setting, and the model-name check's, are
//! left out with it) and `TestHeaderProbeReadings` as far as readings without the probe go.

use serde_json::{Value, json};

use super::*;

fn model_usage(five_h: f64, fable: f64) -> Value {
    json!({"five_hour": {"pct": five_h}, "seven_day": {"pct": 0.0},
           "scoped": [{"name": "Fable", "pct": fable}]})
}

#[test]
fn without_model_setting_the_same_usage_holds() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        ("1", model_usage(5., 100.)),
        ("2", model_usage(5., 30.)),
        ("3", model_usage(5., 60.)),
    ]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.active(), Some(1));
    assert_eq!(h.reasons(), ["below-threshold"]);
}

// -- TestHeaderProbeReadings ------------------------------------------------------------------

#[test]
fn complete_reading_is_a_target() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        ("1", usage(95.)),
        (
            "2",
            json!({"five_hour": {"pct": 5.0}, "seven_day": {"pct": 5.0}}),
        ),
        ("3", usage(40.)),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn weekly_only_active_reading_does_not_fail_over() {
    // No 5h window (Ultra): the 7d reading decides, not three unknown ticks.
    let mut h = Harness::standard();
    for _ in 0..3 {
        let outcome = h.tick_with_usage(&[
            ("1", json!({"seven_day": {"pct": 50.0}})),
            ("2", usage(10.)),
            ("3", usage(10.)),
        ]);
        assert_eq!(outcome, TickOutcome::NoAction);
    }
    assert_eq!(h.active(), Some(1));
    assert_eq!(h.reasons(), ["below-threshold"; 3]);
}

#[test]
fn weekly_only_reading_is_a_target() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        ("1", usage(95.)),
        ("2", json!({"seven_day": {"pct": 0.0}})),
        ("3", usage(40.)),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn rejected_probe_is_not_a_target() {
    let mut h = Harness::standard();
    let refused = json!({"five_hour": {"pct": 5.0}, "seven_day": {"pct": 5.0},
                         "rejected": {"retry_after_s": 600.0, "resets_at": iso(h.now() + 600.)}});
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", refused), ("3", usage(40.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}
