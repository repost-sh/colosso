//! Tests translated from claude-swap's `tests/test_poll_policy.py` (0fe22a8), named after
//! cswap's. cswap's `HALF` rng (0.5) is `rng: 0.5`: a jitter factor of exactly 1.

#![expect(
    clippy::float_cmp,
    reason = "cswap's assertions are exact: intervals are whole seconds scaled by 1.5 or halved"
)]

use serde_json::{Value, json};

use super::*;

const NOW: f64 = 1_000_000.;

fn usage(pct: f64, resets_at: Option<&str>) -> Value {
    let mut window = json!({"pct": pct});
    if let Some(at) = resets_at {
        window["resets_at"] = json!(at);
    }
    json!({"five_hour": window, "seven_day": {"pct": 0.0}})
}

fn usage7(pct5: f64, pct7: f64) -> Value {
    json!({"five_hour": {"pct": pct5}, "seven_day": {"pct": pct7}})
}

/// cswap's `_plan` defaults: a candidate at 10%, threshold 90, no models, no 429.
fn input(new_usage: Option<&Value>) -> PlanInput<'_> {
    PlanInput {
        prev_interval_s: None,
        prev_usage: None,
        new_usage,
        is_active: false,
        threshold: 90.,
        models: &[],
        recent_429: false,
        now: NOW,
        weekly_threshold: None,
        rng: 0.5,
    }
}

fn interval(input: &PlanInput<'_>) -> f64 {
    plan_after_fetch(input).1
}

fn iso(ts: f64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .unwrap()
        .to_rfc3339()
        .replace("+00:00", "Z")
}

// TestIntervalAdaptation

#[test]
fn first_fetch_uses_defaults() {
    let new = usage(10., None);
    let active = PlanInput {
        is_active: true,
        ..input(Some(&new))
    };
    assert_eq!(interval(&active), MIN_INTERVAL_S);
    assert_eq!(interval(&input(Some(&new))), CANDIDATE_DEFAULT_INTERVAL_S);
}

#[test]
fn unmoved_decays_toward_the_ceiling() {
    let (prev, new) = (usage(10., None), usage(10., None));
    let at = |prev_interval_s: f64, is_active: bool| {
        interval(&PlanInput {
            prev_interval_s: Some(prev_interval_s),
            prev_usage: Some(&prev),
            is_active,
            ..input(Some(&new))
        })
    };
    assert_eq!(at(300., false), 450.);
    assert_eq!(at(500., false), CANDIDATE_MAX_INTERVAL_S);
    assert_eq!(at(250., true), ACTIVE_MAX_INTERVAL_S);
}

#[test]
fn movement_halves_floored_at_min() {
    let (prev, new) = (usage(10., None), usage(15., None));
    let at = |prev_interval_s: f64| {
        interval(&PlanInput {
            prev_interval_s: Some(prev_interval_s),
            prev_usage: Some(&prev),
            ..input(Some(&new))
        })
    };
    assert_eq!(at(600.), 300.);
    assert_eq!(at(200.), MIN_INTERVAL_S);
}

#[test]
fn sub_delta_wiggle_is_not_movement() {
    let (prev, new) = (usage(10., None), usage(10.5, None));
    let planned = interval(&PlanInput {
        prev_interval_s: Some(300.),
        prev_usage: Some(&prev),
        ..input(Some(&new))
    });
    assert_eq!(planned, 450.);
}

#[test]
fn unknown_pct_uses_the_default() {
    let planned = interval(&PlanInput {
        prev_interval_s: Some(600.),
        ..input(None)
    });
    assert_eq!(planned, CANDIDATE_DEFAULT_INTERVAL_S);
}

// TestUrgentMode

fn urgent<'a>(prev: &'a Value, new: &'a Value) -> PlanInput<'a> {
    PlanInput {
        prev_interval_s: Some(MIN_INTERVAL_S),
        prev_usage: Some(prev),
        is_active: true,
        ..input(Some(new))
    }
}

#[test]
fn active_moving_in_band_goes_urgent() {
    let (prev, new) = (usage(78., None), usage(82., None));
    assert_eq!(interval(&urgent(&prev, &new)), URGENT_INTERVAL_S);
}

#[test]
fn candidate_never_goes_urgent() {
    let (prev, new) = (usage(78., None), usage(82., None));
    let planned = interval(&PlanInput {
        is_active: false,
        ..urgent(&prev, &new)
    });
    assert_eq!(planned, MIN_INTERVAL_S);
}

#[test]
fn no_movement_no_urgency() {
    let (prev, new) = (usage(78., None), usage(78., None));
    assert!(interval(&urgent(&prev, &new)) > URGENT_INTERVAL_S);
}

#[test]
fn below_the_band_no_urgency() {
    let (prev, new) = (usage(40., None), usage(50., None));
    assert_eq!(interval(&urgent(&prev, &new)), MIN_INTERVAL_S);
}

#[test]
fn recent_429_suppresses_urgency() {
    let (prev, new) = (usage(78., None), usage(82., None));
    let planned = interval(&PlanInput {
        recent_429: true,
        ..urgent(&prev, &new)
    });
    assert_eq!(planned, POST_429_MIN_INTERVAL_S);
}

#[test]
fn urgent_then_unmoved_snaps_back_to_the_floor() {
    // Once movement stops the next interval is the normal floor, never a sub-floor decay.
    let (prev, new) = (usage(78., None), usage(78., None));
    let planned = interval(&PlanInput {
        prev_interval_s: Some(URGENT_INTERVAL_S),
        ..urgent(&prev, &new)
    });
    assert_eq!(planned, MIN_INTERVAL_S);
}

// TestPost429Floor

#[test]
fn recent_429_floors_the_cadence() {
    let (prev, new) = (usage(10., None), usage(10., None));
    let planned = interval(&PlanInput {
        recent_429: true,
        prev_usage: Some(&prev),
        ..input(Some(&new))
    });
    assert!(planned >= POST_429_MIN_INTERVAL_S);
}

#[test]
fn slower_learned_cadence_survives_the_floor() {
    let (prev, new) = (usage(10., None), usage(10., None));
    let planned = interval(&PlanInput {
        recent_429: true,
        prev_interval_s: Some(590.),
        prev_usage: Some(&prev),
        ..input(Some(&new))
    });
    assert!((planned - 590. * POST_429_BACKOFF_MULT).abs() < 1e-9);
    assert!(planned > POST_429_MIN_INTERVAL_S);
}

// TestPost429Aimd

fn after_429(prev_interval_s: Option<f64>, recent_429: bool) -> f64 {
    let (prev, new) = (usage(10., None), usage(10., None));
    interval(&PlanInput {
        recent_429,
        prev_interval_s,
        prev_usage: Some(&prev),
        ..input(Some(&new))
    })
}

#[test]
fn recent_429_multiplicatively_increases_from_prev() {
    let planned = after_429(Some(POST_429_MIN_INTERVAL_S), true);
    assert!(planned > POST_429_MIN_INTERVAL_S);
    assert!((planned - POST_429_MIN_INTERVAL_S * POST_429_BACKOFF_MULT).abs() < 1e-9);
}

#[test]
fn recent_429_ceiling_exceeds_normal_candidate_max() {
    const { assert!(POST_429_MAX_INTERVAL_S > CANDIDATE_MAX_INTERVAL_S) };
    assert_eq!(
        after_429(Some(POST_429_MAX_INTERVAL_S), true),
        POST_429_MAX_INTERVAL_S
    );
}

#[test]
fn no_429_uses_normal_ceiling() {
    assert_eq!(after_429(Some(590.), false), CANDIDATE_MAX_INTERVAL_S);
}

fn converge_trajectory(recent_429: bool) -> Vec<f64> {
    let mut prev = None;
    let mut trajectory = Vec::new();
    for _ in 0..12 {
        let planned = after_429(prev, recent_429);
        trajectory.push(planned);
        prev = Some(planned);
    }
    trajectory
}

#[test]
fn sustained_429_grows_the_interval_to_the_wide_ceiling() {
    let trajectory = converge_trajectory(true);
    assert_eq!(*trajectory.last().unwrap(), POST_429_MAX_INTERVAL_S);
    assert!(trajectory.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(
        trajectory.iter().copied().reduce(f64::max),
        Some(POST_429_MAX_INTERVAL_S)
    );
    for pair in trajectory.windows(2) {
        if pair[1] < POST_429_MAX_INTERVAL_S {
            assert!((pair[1] - pair[0] * POST_429_BACKOFF_MULT).abs() < 1e-9);
        }
    }
}

#[test]
fn without_recency_the_interval_is_capped_at_the_narrow_ceiling() {
    let trajectory = converge_trajectory(false);
    assert_eq!(
        trajectory.iter().copied().reduce(f64::max),
        Some(CANDIDATE_MAX_INTERVAL_S)
    );
    const { assert!(CANDIDATE_MAX_INTERVAL_S < POST_429_MAX_INTERVAL_S) };
}

// TestResetCapping

fn approx(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-6 * right.abs().max(1.)
}

#[test]
fn poll_never_scheduled_past_a_future_reset() {
    let reset_ts = NOW + 90.;
    let new = usage(40., Some(&iso(reset_ts)));
    let (next_poll, planned) = plan_after_fetch(&input(Some(&new)));
    assert!(approx(next_poll, reset_ts + RESET_SLACK_S));
    assert_eq!(planned, CANDIDATE_DEFAULT_INTERVAL_S);
}

#[test]
fn at_limit_keeps_bounded_polling_before_distant_reset() {
    let reset_ts = NOW + 7200.;
    let new = usage(100., Some(&iso(reset_ts)));
    let (next_poll, planned) = plan_after_fetch(&input(Some(&new)));
    assert_eq!(planned, EXHAUSTED_INTERVAL_S);
    assert!(approx(next_poll, NOW + planned));
    assert!(next_poll < reset_ts);
}

#[test]
fn at_limit_poll_is_pulled_to_an_imminent_reset() {
    let reset_ts = NOW + 90.;
    let new = usage(100., Some(&iso(reset_ts)));
    let (next_poll, planned) = plan_after_fetch(&input(Some(&new)));
    assert_eq!(planned, EXHAUSTED_INTERVAL_S);
    assert!(approx(next_poll, reset_ts + RESET_SLACK_S));
}

#[test]
fn at_limit_ignores_non_future_reset() {
    for reset_ts in [NOW - 90., NOW] {
        let new = usage(100., Some(&iso(reset_ts)));
        let (next_poll, planned) = plan_after_fetch(&input(Some(&new)));
        assert_eq!(planned, EXHAUSTED_INTERVAL_S);
        assert!(approx(next_poll, NOW + planned));
    }
}

#[test]
fn active_at_limit_uses_same_bounded_recovery_probe() {
    let reset_ts = NOW + 7200.;
    let new = usage(100., Some(&iso(reset_ts)));
    let (next_poll, planned) = plan_after_fetch(&PlanInput {
        is_active: true,
        ..input(Some(&new))
    });
    assert_eq!(planned, EXHAUSTED_INTERVAL_S);
    assert!(approx(next_poll, NOW + planned));
}

// TestJitter

#[test]
fn jitter_bounds() {
    let new = usage(10., None);
    let (early, _) = plan_after_fetch(&PlanInput {
        rng: 0.,
        ..input(Some(&new))
    });
    let (late, _) = plan_after_fetch(&PlanInput {
        rng: 1.,
        ..input(Some(&new))
    });
    assert!(approx(early, NOW + CANDIDATE_DEFAULT_INTERVAL_S * 0.9));
    assert!(approx(late, NOW + CANDIDATE_DEFAULT_INTERVAL_S * 1.1));
    for _ in 0..100 {
        let draw = random_unit();
        assert!((0. ..1.).contains(&draw));
    }
}

// TestBudgetInvariants: relationships the measured limit asks of the constants.

#[test]
fn sustained_floor_stays_under_the_hourly_cap() {
    const { assert!(MIN_INTERVAL_S >= 180. && SERVE_TTL_S >= 180.) };
}

#[test]
fn edge_backoff_probes_slower_than_capacity_frees() {
    const { assert!(EDGE_BACKOFF_S >= 300.) };
}

#[test]
fn post_429_floor_covers_the_saturation_horizon() {
    const { assert!(RECENT_429_WINDOW_S >= 3600. && POST_429_MIN_INTERVAL_S >= MIN_INTERVAL_S) };
}

#[test]
fn urgent_episode_alone_fits_inside_the_window_cap() {
    const { assert!(ESCALATION_MARGIN_PCT / MOVEMENT_DELTA_PCT < 27.) };
}

// TestPerWindowUrgency

fn active(prev: &Value, new: &Value, threshold: f64, weekly: Option<f64>, models: &[&str]) -> f64 {
    interval(&PlanInput {
        prev_interval_s: Some(MIN_INTERVAL_S),
        prev_usage: Some(prev),
        is_active: true,
        threshold,
        weekly_threshold: weekly,
        models,
        ..input(Some(new))
    })
}

#[test]
fn the_existing_urgent_plan_is_unchanged_with_weekly_unset() {
    let planned = active(&usage(78., None), &usage(82., None), 90., None, &[]);
    assert_eq!(planned, URGENT_INTERVAL_S);
}

#[test]
fn a_non_max_window_rising_in_its_band_goes_urgent() {
    // 5h 76 -> 78 under a flat 7d 85 max.
    let planned = active(&usage7(76., 85.), &usage7(78., 85.2), 90., None, &[]);
    assert_eq!(planned, URGENT_INTERVAL_S);
}

#[test]
fn s10_the_5h_rising_under_a_flat_weekly_binding() {
    let (prev, new) = (usage7(80., 94.), usage7(84., 94.));
    assert_eq!(active(&prev, &new, 85., Some(97.), &[]), URGENT_INTERVAL_S);
    // A candidate never goes urgent; its unmoved max decays as before.
    let planned = interval(&PlanInput {
        prev_interval_s: Some(300.),
        prev_usage: Some(&prev),
        threshold: 85.,
        weekly_threshold: Some(97.),
        ..input(Some(&new))
    });
    assert_eq!(planned, 450.);
}

#[test]
fn s5_the_weekly_window_burning_toward_its_own_line() {
    let (prev, new) = (usage7(20., 90.5), usage7(20., 96.9));
    assert_eq!(active(&prev, &new, 85., Some(97.), &[]), URGENT_INTERVAL_S);
    let halved = interval(&PlanInput {
        prev_interval_s: Some(600.),
        prev_usage: Some(&prev),
        threshold: 85.,
        weekly_threshold: Some(97.),
        ..input(Some(&new))
    });
    assert_eq!(halved, 300.);
}

#[test]
fn a_rise_outside_its_own_band_is_not_urgent() {
    let planned = active(&usage7(60., 94.), &usage7(62., 94.), 85., Some(97.), &[]);
    assert_eq!(planned, 270.);
}

#[test]
fn a_window_dropping_into_the_band_is_not_urgent() {
    assert_eq!(
        active(&usage(90., None), &usage(80., None), 90., None, &[]),
        MIN_INTERVAL_S
    );
    assert_eq!(
        active(&usage7(90., 80.), &usage7(60., 80.), 90., None, &[]),
        MIN_INTERVAL_S
    );
}

#[test]
fn a_binding_flip_is_not_movement() {
    let planned = active(&usage7(84., 95.9), &usage7(84.1, 95.9), 85., Some(97.), &[]);
    assert_eq!(planned, 270.);
}

#[test]
fn a_window_present_on_one_side_only_is_ignored() {
    let prev = json!({"five_hour": {"pct": 10.0}});
    assert_eq!(
        active(&prev, &usage7(10., 85.), 90., None, &[]),
        MIN_INTERVAL_S
    );
}

#[test]
fn a_scoped_window_rises_against_the_weekly_line() {
    let reading = |fable: f64| {
        json!({
            "five_hour": {"pct": 10.0},
            "seven_day": {"pct": 10.0},
            "scoped": [{"name": "Fable", "pct": fable}],
        })
    };
    let (prev, new) = (reading(80.), reading(83.));
    assert_eq!(
        active(&prev, &new, 85., Some(97.), &["Fable"]),
        URGENT_INTERVAL_S
    );
    // Unconfigured, the scoped window is invisible to the planner too.
    assert_eq!(active(&prev, &new, 85., Some(97.), &[]), 270.);
}
