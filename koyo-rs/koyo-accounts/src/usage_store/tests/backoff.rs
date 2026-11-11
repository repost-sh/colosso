//! From claude-swap's `tests/test_usage_store.py` (0fe22a8):
//! `TestBackoff`, to the spec's two numbers: the curve caps at 900 s (cswap: 600), and the
//! `Retry-After` margin applies over 600 s, its own constant (cswap: `> BACKOFF_CAP_S`).

#![expect(
    clippy::float_cmp,
    reason = "cswap's assertions are exact: waits are whole seconds, sums and doublings of them"
)]

use serde_json::{Value, json};

use super::*;

fn wait(failures: i64, retry_after_s: Option<f64>) -> f64 {
    failure_backoff_s(failures, retry_after_s, true)
}

/// The spec's numbers themselves.
#[test]
fn the_spec_sets_the_cap_and_keeps_the_margin_threshold() {
    assert_eq!(BACKOFF_BASE_S, 30.);
    assert_eq!(BACKOFF_CAP_S, 900.);
    assert_eq!(RETRY_AFTER_MARGIN_ABOVE_S, 600.);
    assert_eq!(RETRY_AFTER_MARGIN_S, 900.);
    assert_eq!(RETRY_AFTER_FLOOR_CAP_S, 4500.);
    // An ask between the old cap and the new one takes the margin, as cswap's measured rule.
    assert_eq!(wait(1, Some(601.)), 1501.);
    assert_eq!(wait(1, Some(900.)), 1800.);
}

/// cswap expects `[30, 60, 120, 240, 480, 600, 600]`; the spec caps at 900.
#[test]
fn exponential_backoff() {
    let h = Harness::new();
    for (i, want) in [30., 60., 120., 240., 480., 900., 900.]
        .into_iter()
        .enumerate()
    {
        h.record("1", err("http-500"));
        let entry = h.entry("1");
        assert_eq!(entry.consecutive_failures, i as i64 + 1);
        assert!(approx(entry.backoff_until.unwrap(), h.now() + want), "{i}");
        h.advance(want + 1.);
    }
}

#[test]
fn backoff_cap() {
    assert_eq!(wait(50, None), BACKOFF_CAP_S);
}

#[test]
fn huge_failure_count_does_not_overflow() {
    assert_eq!(wait(1025, None), BACKOFF_CAP_S);
    assert_eq!(wait(10_000, Some(90.)), BACKOFF_CAP_S);
    assert_eq!(wait(i64::MAX, None), BACKOFF_CAP_S);
}

#[test]
fn record_failure_on_saturated_counter_does_not_raise() {
    let h = Harness::new();
    h.write_raw(
        &json!({
            "schemaVersion": 2,
            "accounts": {"1": {
                "email": "a@x.com", "consecutiveFailures": 1024, "lastError": "refresh-failed",
            }},
        })
        .to_string(),
    );
    h.record("1", err("refresh-failed"));
    let entry = h.entry("1");
    assert_eq!(entry.consecutive_failures, 1025);
    assert!(approx(
        entry.backoff_until.unwrap(),
        h.now() + BACKOFF_CAP_S
    ));
}

#[test]
fn retry_after_is_the_floor() {
    let h = Harness::new();
    h.record("1", err_after("http-429", 90.));
    let entry = h.entry("1");
    // The first failure computes 30 s, but the server asked 90 s: honoured as the floor, with
    // no margin under the threshold.
    assert!(approx(entry.backoff_until.unwrap(), h.now() + 90.));
    assert!(entry.in_backoff(h.now() + 89.));
    assert!(!entry.in_backoff(h.now() + 91.));
}

#[test]
fn own_curve_may_exceed_retry_after() {
    assert_eq!(wait(5, Some(10.)), 480.);
    assert_eq!(BACKOFF_BASE_S * 2f64.powi(4), 480.);
}

/// cswap expects `[300, 300, 300, 300, 480, 600, 600]`; the spec caps at 900.
#[test]
fn edge_429_backoff_floors_at_edge_backoff() {
    let h = Harness::new();
    for (i, want) in [300., 300., 300., 300., 480., 900., 900.]
        .into_iter()
        .enumerate()
    {
        h.record("1", err_after("http-429", 0.));
        let entry = h.entry("1");
        assert_eq!(entry.consecutive_failures, i as i64 + 1);
        assert!(approx(entry.backoff_until.unwrap(), h.now() + want), "{i}");
        h.advance(want + 1.);
    }
}

#[test]
fn a_non_429_retry_after_zero_does_not_take_the_saturated_edge() {
    let h = Harness::new();
    h.record("1", err_after("http-503", 0.));
    // The plain curve's 30 s, not the 429-only edge floor.
    assert!(approx(h.entry("1").backoff_until.unwrap(), h.now() + 30.));
}

#[test]
fn retry_after_floor_is_capped() {
    assert_eq!(wait(1, Some(50_000.)), RETRY_AFTER_FLOOR_CAP_S);
}

#[test]
fn hour_scale_retry_after_honored() {
    assert_eq!(wait(1, Some(3600.)), 4500.);
    const { assert!(RETRY_AFTER_FLOOR_CAP_S >= 4500.) };
}

#[test]
fn hour_scale_margin_clears_the_measured_re_block_band() {
    assert!(wait(1, Some(3600.)) - 3600. >= 900.);
}

/// cswap's boundary is `BACKOFF_CAP_S` (600 there); here it is its own 600 s threshold, and an
/// ask at the spec's 900 s cap takes the margin.
#[test]
fn a_short_accurate_block_is_not_inflated() {
    assert_eq!(wait(1, Some(300.)), 300.);
    assert_eq!(wait(1, Some(3600.)) - 3600., 900.);
    let threshold = RETRY_AFTER_MARGIN_ABOVE_S;
    assert_eq!(wait(1, Some(threshold)), threshold);
    assert_eq!(wait(1, Some(threshold + 1.)), threshold + 1. + 900.);
}

#[test]
fn margin_survives_a_mid_block_observation() {
    for remaining in [3600., 1800., 900.] {
        let overshoot = wait(1, Some(remaining)) - remaining;
        assert!(
            overshoot >= 900.,
            "Retry-After {remaining} lands {overshoot} past"
        );
    }
}

#[test]
fn a_429_wait_is_the_deadline_plus_the_margin() {
    for ask in [3601., 3600., 4000.] {
        let expected = (ask + RETRY_AFTER_MARGIN_S).min(RETRY_AFTER_FLOOR_CAP_S);
        assert_eq!(wait(1, Some(ask)), expected, "ask {ask}");
    }
}

#[test]
fn the_cap_sits_inside_the_trust_it_relies_on() {
    assert_eq!(RETRY_AFTER_FLOOR_CAP_S, 3600. + RETRY_AFTER_MARGIN_S);
    const { assert!(RETRY_AFTER_FLOOR_CAP_S <= RATE_LIMIT_TRUST_MAX_AGE_S) };
    for ask in [3600., 4500., 50_000., 86_400., f64::INFINITY] {
        assert!(
            wait(1, Some(ask)) <= RATE_LIMIT_TRUST_MAX_AGE_S,
            "ask {ask}"
        );
    }
}

#[test]
fn each_arm_is_bounded_by_the_ceiling_its_own_trust_uses() {
    for ask in [3601., 4500., 7200., 86_400., f64::INFINITY] {
        assert!(
            failure_backoff_s(1, Some(ask), false) <= TRUST_MAX_AGE_S,
            "ask {ask}"
        );
    }
    for ask in [4500., 7200., 50_000., 86_400., f64::INFINITY] {
        assert!(
            failure_backoff_s(1, Some(ask), true) <= RATE_LIMIT_TRUST_MAX_AGE_S,
            "ask {ask}"
        );
    }
}

#[test]
fn a_soon_resetting_window_can_end_trust_before_the_429_wait_releases() {
    let h = Harness::new();
    let reading = json!({
        "five_hour": {"pct": 25.0, "resets_at": iso(h.now() + 1800.)},
        "seven_day": {"pct": 10.0, "resets_at": iso(h.now() + 100. * 3600.)},
    });
    assert!(!crate::oauth::relevant_windows(Some(&reading), &[]).is_empty());
    h.record("1", FetchRecord::success(Some(reading.clone())));
    h.record("1", err_after("http-429", 3600.));
    let wait = wait(1, Some(3600.));
    assert_eq!(wait, 4500.);

    h.advance(1799.);
    let entry = h.entry("1");
    assert!(entry.in_backoff(h.now()));
    assert_eq!(decision(&entry), Some(&reading));

    h.advance(2.);
    let entry = h.entry("1");
    assert!(entry.in_backoff(h.now()));
    assert_eq!(entry.decision_value(), None);

    h.advance(wait - 1801.);
    let entry = h.entry("1");
    assert!(!entry.in_backoff(h.now()));
    assert_eq!(entry.decision_value(), None);
}

#[test]
fn consecutive_blocks_go_blind_because_fetched_at_only_moves_on_success() {
    let h = Harness::new();
    let far = iso(1e9);
    let last_good = json!({
        "five_hour": {"pct": 25.0, "resets_at": far},
        "seven_day": {"pct": 10.0, "resets_at": far},
    });
    assert!(!crate::oauth::relevant_windows(Some(&last_good), &[]).is_empty());
    assert!(earliest_reset(Some(&last_good), &[]).is_some());
    h.record("1", FetchRecord::success(Some(last_good)));
    let fetched_at = h.entry("1").fetched_at;

    let mut blind_per_block = Vec::new();
    let mut last_wait = 0.;
    for _ in 0..3 {
        h.record("1", err_after("http-429", 3600.));
        let entry = h.entry("1");
        assert_eq!(
            entry.fetched_at, fetched_at,
            "a failed record moved fetchedAt"
        );
        let block_end = entry.backoff_until.unwrap();
        last_wait = block_end - h.now();
        let mut blind = 0.;
        while h.now() < block_end {
            if h.entry("1").decision_value().is_none() {
                blind = block_end - h.now();
                break;
            }
            h.advance(60.);
        }
        blind_per_block.push(blind);
        h.advance((block_end - h.now()).max(0.));
    }
    assert!(approx(last_wait, 4500.));
    assert_eq!(blind_per_block[0], 0.);
    assert!(blind_per_block[1] > 0.);
    assert!(approx(blind_per_block[2], last_wait));
}

#[test]
fn the_margin_never_lifts_the_floor_cap() {
    assert!(wait(1, Some(86_400.)) <= RETRY_AFTER_FLOOR_CAP_S);
}

#[test]
fn short_asks_stay_on_our_own_curve() {
    assert_eq!(wait(1, Some(90.)), 90.);
    assert_eq!(wait(10_000, Some(90.)), BACKOFF_CAP_S);
}

#[test]
fn measured_burst_block_honored_exactly() {
    assert_eq!(wait(1, Some(300.)), 300.);
}

/// The time a row is both in backoff and untrusted after one failure at `age_at_fail`.
fn blind_window(age_at_fail: f64, ask: f64, error: &str) -> f64 {
    let h = Harness::new();
    let ident = one_identity("1", "a@example.com", "");
    let reading = FetchRecord::success(Some(json!({"five_hour": {"pct": 1.0}})));
    record_as(&h.store, "1", reading, &ident);
    h.advance(age_at_fail);
    record_as(&h.store, "1", err_after(error, ask), &ident);
    let park_end = entry_as(&h.store, "1", &ident).backoff_until.unwrap();
    let mut at = h.now();
    let mut blind_start = None;
    while at < park_end {
        h.set(at);
        let entry = entry_as(&h.store, "1", &ident);
        if entry.in_backoff(at) && entry.decision_value().is_none() {
            blind_start = Some(at);
            break;
        }
        at += 1.;
    }
    blind_start.map_or(0., |start| park_end - start)
}

#[test]
fn park_bound_blind_window_equals_age_at_failure() {
    // The non-429 arm, where the park cap equals TRUST_MAX_AGE_S: blind == age at failure.
    for (age, ask, expected) in [
        (0., 5000., 0.),
        (1., 5000., 1.),
        (120., 5000., 120.),
        (300., 5000., 300.),
        (1800., 5000., 1800.),
        (3599., 5000., 3599.),
        (300., 4000., 300.),
        (300., 3600., 300.),
        (300., 600., 0.),
    ] {
        let blind = blind_window(age, ask, "http-500");
        assert!(
            (blind - expected).abs() <= 2.,
            "age {age} ask {ask}: {blind}"
        );
    }
    // The 429 arm: blind = age - 2700, clamped to [0, park].
    for (age, ask, expected) in [
        (0., 5000., 0.),
        (2701., 5000., 1.),
        (3600., 5000., 900.),
        (5000., 5000., 2300.),
    ] {
        let blind = blind_window(age, ask, "http-429");
        assert!(
            (blind - expected).abs() <= 2.,
            "[429] age {age} ask {ask}: {blind}"
        );
    }
}

#[test]
fn rejection_backoff_is_bounded_like_an_ask() {
    let rejected = |retry_after: Value| json!({"rejected": {"retry_after_s": retry_after}});
    assert_eq!(
        rejection_backoff_until(Some(&rejected(json!(600.0))), START),
        Some(START + 600.)
    );
    assert_eq!(
        rejection_backoff_until(Some(&rejected(json!(1e9))), START),
        Some(START + RETRY_AFTER_FLOOR_CAP_S)
    );
    assert_eq!(
        rejection_backoff_until(Some(&rejected(json!(0))), START),
        None
    );
    assert_eq!(
        rejection_backoff_until(Some(&rejected(Value::Null)), START),
        None
    );
    assert_eq!(rejection_backoff_until(None, START), None);
}
