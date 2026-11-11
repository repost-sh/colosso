//! cswap's `TestAllAboveHeadroomDoesNotFlap`, third part, through `tick()`: the release's axes
//! (a reset nearer, a reset gained) and the failover departure, which records no baseline and
//! keeps its own two legs. Its cases of a record with no snapshot at all (cswap's pre-upgrade
//! shape) are left out with that reading.

use serde_json::Value;

use super::no_return::DaysOut;
use super::*;

#[test]
fn a_reset_that_crept_nearer_is_not_a_recovery() {
    let mut h = Harness::fleet(2, defaults());
    let mut days = DaysOut::default();
    let depart = at(&h, 500. * 3600.);
    let first = [
        ("1", usage_r(96., &depart)),
        ("2", usage_r(92., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(3612.);
    let mut outcomes = Vec::new();
    for _ in 0..10 {
        // A minute nearer than at departure: jitter, not a recovery.
        let fleet = [
            ("1", usage_r(96., &at(&h, 500. * 3600. - 3612. - 60.))),
            ("2", usage_r(98., &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(!outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn an_unschedulable_account_that_gained_a_reset_has_recovered() {
    let mut h = Harness::fleet(2, defaults());
    let mut days = DaysOut::default();
    let first = [("1", usage(96.)), ("2", usage_r(92., &days.at(&h, 400.)))];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert!(
        h.state().get("leftRecoveryAt").is_none(),
        "premise: no recovery stored"
    );
    h.advance(3612.);
    let mut outcomes = Vec::new();
    for _ in 0..10 {
        let fleet = [
            ("1", usage_r(96., &at(&h, 3600.))),
            ("2", usage_r(98., &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

/// Three unreadable ticks fail over 1 → 2 (2 at 4%, resetting at `reset`); cswap's failover
/// departure.
fn failed_over(reset: &dyn Fn(&Harness, &mut DaysOut) -> String) -> (Harness, DaysOut) {
    let mut h = Harness::fleet(2, defaults());
    let mut days = DaysOut::default();
    let mut outcome = TickOutcome::NoAction;
    for _ in 0..3 {
        let reset = reset(&h, &mut days);
        outcome = h.tick_with_usage(&[("1", Value::Null), ("2", usage_r(4., &reset))]);
        h.advance(60.);
    }
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    let state = h.state();
    assert_eq!(state["leftTrigger"], "failover");
    assert!(state.get("leftHeadroom").is_none() && state.get("leftRecoveryAt").is_none());
    (h, days)
}

fn days_400(h: &Harness, days: &mut DaysOut) -> String {
    days.at(h, 400.)
}

fn at_400h(h: &Harness, _: &mut DaysOut) -> String {
    at(h, 400. * 3600.)
}

#[test]
fn a_failover_departure_does_not_disarm_the_bar() {
    let (mut h, mut days) = failed_over(&days_400);
    let frozen = days.at(&h, 500.);
    h.advance(301.);
    let mut outcomes = Vec::new();
    for active in [98., 98.2, 98.4, 99., 99.5, 99.9] {
        let fleet = [
            ("1", usage_r(96., &frozen)),
            ("2", usage_r(active, &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(!outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn a_failover_departure_still_unreadable_does_not_crash_or_release() {
    let (mut h, mut days) = failed_over(&days_400);
    h.advance(301.);
    let mut outcomes = Vec::new();
    for _ in 0..10 {
        let fleet = [("1", Value::Null), ("2", usage_r(98., &days.at(&h, 400.)))];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(!outcomes.contains(&TickOutcome::Error), "{outcomes:?}");
    assert!(!outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

fn returns(
    h: &mut Harness,
    days: &mut DaysOut,
    one: f64,
    one_hours: f64,
    two: f64,
    ticks: usize,
) -> Vec<TickOutcome> {
    let mut outcomes = Vec::new();
    for _ in 0..ticks {
        let fleet = [
            ("1", usage_r(one, &days.at(h, one_hours))),
            ("2", usage_r(two, &days.at(h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    outcomes
}

#[test]
fn a_failover_departure_releases_once_the_peer_is_readable_again() {
    let (mut h, mut days) = failed_over(&days_400);
    h.advance(301.);
    let outcomes = returns(&mut h, &mut days, 0., 10., 95., 10);
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn a_failover_departure_releases_a_healthy_but_not_near_full_peer() {
    let (mut h, mut days) = failed_over(&days_400);
    h.advance(301.);
    let outcomes = returns(&mut h, &mut days, 30., 10., 98., 10);
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn a_failover_departure_releases_when_the_peer_resets_first_in_the_all_spent_regime() {
    let (mut h, mut days) = failed_over(&at_400h);
    h.advance(301.);
    let mut outcomes = Vec::new();
    for _ in 0..8 {
        let fleet = [
            ("1", usage_r(96., &at(&h, 300.))),
            ("2", usage_r(98., &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn the_recovery_leg_requires_the_actives_reset_to_be_known_not_merely_absent() {
    type Row = fn(&Harness, &mut DaysOut) -> Value;
    let cases: [(&str, Row, TickOutcome, u32); 3] = [
        (
            "active reset unreported",
            |_, _| usage(98.),
            TickOutcome::Blocked,
            2,
        ),
        (
            "control: active resets in 500 h",
            |h, days| usage_r(98., &days.at(h, 500.)),
            TickOutcome::Switched,
            1,
        ),
        (
            "control: active resets in 10 min",
            |h, _| usage_r(98., &at(h, 600.)),
            TickOutcome::Blocked,
            2,
        ),
    ];
    for (label, active_row, expected, expected_active) in cases {
        let (mut h, mut days) = failed_over(&at_400h);
        h.advance(301.);
        let active = active_row(&h, &mut days);
        let outcome =
            h.tick_with_usage(&[("1", usage_r(96., &at(&h, 400. * 3600.))), ("2", active)]);
        assert_eq!(
            (outcome, h.active()),
            (expected, Some(expected_active)),
            "{label}"
        );
    }
}

#[test]
fn the_isfinite_guard_must_not_hold_when_a_near_peer_is_available() {
    type Row = fn(&Harness) -> Value;
    type Reset = fn(&Harness) -> String;
    let cases: [(&str, Row, Reset, TickOutcome, u32); 4] = [
        (
            "active 400 h out, peer ~50 min",
            |h| usage_r(98., &at(h, 400. * 3600.)),
            |h| at(h, 3000.),
            TickOutcome::Switched,
            1,
        ),
        (
            "active 400 h out, peer 60 s sooner",
            |h| usage_r(98., &at(h, 400. * 3600.)),
            |h| at(h, 400. * 3600. - 60.),
            TickOutcome::Blocked,
            2,
        ),
        (
            "active with no reset, peer ~50 min",
            |_| usage(98.),
            |h| at(h, 3000.),
            TickOutcome::Switched,
            1,
        ),
        (
            "active reset past, peer ~50 min",
            |h| usage_r(98., &at(h, -3600.)),
            |h| at(h, 3000.),
            TickOutcome::Switched,
            1,
        ),
    ];
    for (label, active_row, peer_reset, expected, expected_active) in cases {
        let (mut h, _) = failed_over(&at_400h);
        h.advance(301.);
        let fleet = [("1", usage_r(96., &peer_reset(&h))), ("2", active_row(&h))];
        let outcome = h.tick_with_usage(&fleet);
        assert_eq!(
            (outcome, h.active()),
            (expected, Some(expected_active)),
            "{label}"
        );
    }
}

#[test]
fn a_failover_hold_still_escapes_at_limit() {
    let (mut h, mut days) = failed_over(&days_400);
    h.advance(301.);
    let held = returns(&mut h, &mut days, 96., 500., 98., 3);
    assert!(
        !held.contains(&TickOutcome::Switched),
        "premise: the hold blocks the return: {held:?}"
    );
    let fleet = [
        ("1", usage_r(0., &days.at(&h, 500.))),
        ("2", usage_r(100., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn failover_peer_frozen_active_burning_holds() {
    let (mut h, _) = failed_over(&at_400h);
    h.advance(301.);
    let fleet = [
        ("1", usage_r(96., &at(&h, 500. * 3600.))),
        ("2", usage_r(98.2, &at(&h, 400. * 3600.))),
    ];
    assert_ne!(h.tick_with_usage(&fleet), TickOutcome::Switched);
}

#[test]
fn end_to_end_failover_departure_records_its_own_trigger() {
    let mut h = Harness::fleet(2, defaults());
    let mut outcome = TickOutcome::NoAction;
    for _ in 0..3 {
        outcome = h.tick_with_usage(&[("1", Value::Null), ("2", usage(4.))]);
        h.advance(60.);
    }
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.state()["leftTrigger"], "failover");
}

#[test]
fn the_release_fires_when_the_barred_account_recovered() {
    let mut h = Harness::fleet(2, defaults());
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(96., &days.at(&h, 500.))),
        ("2", usage_r(92., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    h.advance(3612.);
    let outcomes = returns(&mut h, &mut days, 0., 500., 98., 10);
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn the_ratio_release_changes_where_the_engine_lands() {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(92., &days.at(&h, 500.))),
        ("2", usage_r(10., &days.at(&h, 400.))),
        ("3", usage_r(70., &days.at(&h, 300.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(301.);
    let second = [
        ("1", usage_r(20., &days.at(&h, 500.))), // barred, 80 points
        ("2", usage_r(90., &days.at(&h, 400.))), // active, 10 points
        ("3", usage_r(70., &days.at(&h, 300.))), // 30 points
    ];
    assert_eq!(h.tick_with_usage(&second), TickOutcome::Switched);
    assert_eq!(
        h.active(),
        Some(1),
        "8x the active's headroom is a move on its own merits"
    );
}

#[test]
fn a_materially_better_peer_still_wins() {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let fleet = [
        ("1", usage_r(98., &days.at(&h, 109.))),
        ("2", usage_r(90., &days.at(&h, 80.))),
        ("3", usage_r(99., &days.at(&h, 50.))),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn left_snapshot_uses_the_ranking_now_not_a_fresh_clock_read() {
    // The account left resets 100 s after the ranking's instant; the clock then moves 200 s
    // before the switch (here: while the target's credential is read). A second clock read
    // would see the reset past and record null.
    let mut h = Harness::fleet(2, defaults());
    let ranking_now = h.now();
    let fleet = [
        ("1", usage_r(95., &iso(ranking_now + 100.))),
        ("2", usage(10.)),
    ];
    h.host.lock().advance_on_read = Some(200.);
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert!(
        h.now() > ranking_now + 100.,
        "premise: the clock passed the reset"
    );
    assert_eq!(h.state()["leftRecoveryAt"], ranking_now + 100.);
}
