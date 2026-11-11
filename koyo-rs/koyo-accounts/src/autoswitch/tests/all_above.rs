//! cswap's `TestEveryAccountAboveThreshold`, `TestASoonerResetNeverBuysAMove`,
//! `TestAHeadroomTieTakesTheSoonerReset`, `TestASpentFleetRanksByRoomToo`,
//! `TestEscapeBeforeTheLimitLands` and `TestReviewFindings202`: with nothing under the line, go
//! to the account with the most room, by a margin. `TestAllAboveRoomIsDistanceToTheLimit` sets
//! a weekly line in every case and is left out with it.

use serde_json::{Value, json};

use super::super::windows::Reading;
use super::*;

// -- TestEveryAccountAboveThreshold ----------------------------------------------------------

#[test]
fn moves_to_the_account_with_the_most_room() {
    // The measured shape: active 99, peers 100 and 95; 3 is the only one with room.
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 7200.)),
        ("2", r(100., &h, 600.)),
        ("3", r(95., &h, 480.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn most_room_wins_over_a_sooner_reset() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 3600.)),
        ("2", r(91., &h, 3. * 3600.)),
        ("3", r(97., &h, 300.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn the_measured_late_switch_lands_on_the_most_room() {
    // 2026-10-16, threshold 80 (cswap runs it under consume-first; the all-above ranking and its
    // capped margin are every strategy's). 3 has 19 points against the active's 10, short of the
    // uncapped ratio's 20.
    let mut h = Harness::numbered(5, threshold(80.));
    let fleet = [
        ("1", usage57(0., None, 90., Some(&at(&h, 3. * 86400.)))),
        (
            "2",
            usage57(99., Some(&at(&h, 1200.)), 33., Some(&at(&h, 5. * 86400.))),
        ),
        (
            "3",
            usage57(81., Some(&at(&h, 7200.)), 11., Some(&at(&h, 6. * 86400.))),
        ),
        (
            "4",
            usage57(36., Some(&at(&h, 10800.)), 84., Some(&at(&h, 2. * 86400.))),
        ),
        ("5", usage57(0., None, 87., Some(&at(&h, 86400.)))),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn a_failover_ranks_by_room_when_every_candidate_is_above() {
    // Failover can't read the active, so the candidates alone decide nothing is under a line
    // (cswap runs it under consume-first).
    let mut h = Harness::numbered(3, defaults());
    let mut outcome = TickOutcome::NoAction;
    for _ in 0..3 {
        outcome = h.tick_with_usage(&[
            ("1", Value::Null),
            ("2", usage7(97., 50., Some(R_SOON))),
            ("3", usage7(91., 50., Some(R_LATEST))),
        ]);
        h.advance(60.);
    }
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn a_single_healthy_peer_still_wins_normally() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 3600.)),
        ("2", r(95., &h, 60.)),
        ("3", r(20., &h, 5. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn below_threshold_is_untouched() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn all_at_limit_still_reports_exhausted() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(100., &h, 600.)),
        ("2", r(100., &h, 300.)),
        ("3", r(100., &h, 900.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn an_unknown_reset_loses_a_headroom_tie() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 3600.)),
        ("2", usage(95.)),
        ("3", r(95., &h, 600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

#[test]
fn does_not_flap_between_two_near_equal_accounts() {
    // One point of room isn't worth a move: the target burns it back within a poll or two.
    let mut h = Harness::standard();
    let first = h.tick_with_usage(&[("1", usage(96.)), ("2", usage(95.)), ("3", usage(100.))]);
    assert_eq!(first, TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn the_margin_is_capped_at_spent_headroom_points() {
    // Above SPENT_HEADROOM_PCT the margin is that many points, not twice the active.
    for (peer, moves) in [(87., true), (87.1, false)] {
        let mut h = Harness::numbered(2, threshold(80.));
        let outcome = h.tick_with_usage(&[("1", usage(90.)), ("2", usage(peer))]);
        assert_eq!(outcome == TickOutcome::Switched, moves, "{peer}");
        assert_eq!(h.active(), Some(if moves { 2 } else { 1 }));
    }
}

#[test]
fn a_recovered_account_clears_the_bar_at_the_capped_margin() {
    // The no-return release asks for the ranking's capped margin on both sides of the cap.
    for (departure, recovery) in [
        ([95., 85., 92.], [81., 90., 87.]),
        ([98.5, 97., 99.], [95.5, 98., 96.]),
    ] {
        let mut h = Harness::numbered(3, threshold(80.));
        let fleet = |pcts: [f64; 3]| {
            [
                ("1", usage(pcts[0])),
                ("2", usage(pcts[1])),
                ("3", usage(pcts[2])),
            ]
        };
        assert_eq!(h.tick_with_usage(&fleet(departure)), TickOutcome::Switched);
        assert_eq!(h.active(), Some(2));
        h.advance(400.);
        assert_eq!(h.tick_with_usage(&fleet(recovery)), TickOutcome::Switched);
        assert_eq!(h.active(), Some(1), "{departure:?} {recovery:?}");
    }
}

#[test]
fn below_the_cap_the_ratio_decides() {
    for (peer, moves) in [(98., true), (98.1, false)] {
        let mut h = Harness::standard();
        let outcome =
            h.tick_with_usage(&[("1", usage(99.)), ("2", usage(peer)), ("3", usage(100.))]);
        assert_eq!(outcome == TickOutcome::Switched, moves, "{peer}");
        assert_eq!(h.active(), Some(if moves { 2 } else { 1 }));
    }
}

#[test]
fn at_limit_trigger_still_ignores_the_landing_rule() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(100., &h, 60.)),
        ("2", r(30., &h, 86400.)),
        ("3", r(95., &h, 120.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    assert_eq!(h.trigger(), Trigger::AtLimit);
}

// -- TestASoonerResetNeverBuysAMove -----------------------------------------------------------

#[test]
fn a_sooner_reset_on_less_room_does_not_move() {
    type Shape = fn(&Harness) -> Vec<(&'static str, Value)>;
    let shapes: [(&str, Shape); 5] = [
        ("minutes-away", |h| {
            vec![
                ("1", r(91., h, 7200.)),
                ("2", r(94., h, 1800.)),
                ("3", r(98., h, 480.)),
            ]
        }),
        ("days-away", |h| {
            vec![
                ("1", r(91., h, 109. * 3600.)),
                ("2", r(94., h, 80. * 3600.)),
                ("3", r(98., h, 50. * 3600.)),
            ]
        }),
        ("weekly-bound-active", |h| {
            vec![
                ("1", usage7(10., 96., Some(&at(h, 109. * 3600.)))),
                ("2", r(98., h, 480.)),
                ("3", r(99., h, 90. * 3600.)),
            ]
        }),
        ("unknown-active-reset", |h| {
            vec![
                ("1", usage(91.)),
                ("2", r(98., h, 80. * 3600.)),
                ("3", r(99., h, 50. * 3600.)),
            ]
        }),
        ("spent-band", |h| {
            vec![
                ("1", r(97.5, h, 500. * 3600.)),
                ("2", r(98., h, 490. * 3600.)),
                ("3", usage(100.)),
            ]
        }),
    ];
    for (name, shape) in shapes {
        let mut h = Harness::standard();
        let fleet = shape(&h);
        let outcome = h.tick_with_usage(&fleet);
        assert_eq!(
            h.active(),
            Some(1),
            "{name}: traded room for a sooner reset"
        );
        assert_ne!(outcome, TickOutcome::Switched, "{name}");
    }
}

#[test]
fn a_peer_with_real_headroom_wins_whatever_its_reset() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(97., &h, 50. * 3600.)),
        ("2", r(91., &h, 109. * 3600.)),
        ("3", r(98., &h, 60. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn an_unreadable_peer_does_not_veto_the_escape() {
    // The census counts the measured candidates: a sentinel row (headroom unknown) would keep the
    // landing rule on with nowhere to land.
    let mut h = Harness::fleet(4, defaults());
    let fleet = [
        ("1", r(99., &h, 109. * 3600.)),
        ("2", r(95., &h, 80. * 3600.)),
        ("3", r(99., &h, 50. * 3600.)),
        ("4", json!(USAGE_TOKEN_EXPIRED)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn equal_headroom_takes_the_sooner_reset() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(96., &h, 300. * 3600.)),
        ("2", r(92., &h, 500. * 3600.)),
        ("3", r(92., &h, 5. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

// -- TestASpentFleetRanksByRoomToo ------------------------------------------------------------

#[test]
fn an_even_spent_fleet_stays_put() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 109. * 3600.)),
        ("2", r(99., &h, 80. * 3600.)),
        ("3", r(99., &h, 50. * 3600.)),
    ];
    assert_ne!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn real_headroom_still_beats_a_sooner_reset() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(98., &h, 109. * 3600.)),
        ("2", r(90., &h, 80. * 3600.)),
        ("3", r(99., &h, 50. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn a_spent_fleet_takes_the_most_room() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99.5, &h, 300. * 3600.)),
        ("2", r(99.5, &h, 10. * 3600.)),
        ("3", r(99., &h, 500. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

// -- TestEscapeBeforeTheLimitLands ------------------------------------------------------------

#[test]
fn at_99_the_proactive_path_already_escapes() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 109. * 3600.)),
        ("2", r(70., &h, 80. * 3600.)),
        ("3", r(100., &h, 50. * 3600.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    // At-limit stays bound to headroom <= 0: it skips the anti-flap margin proactive owns.
    assert_eq!(h.trigger(), Trigger::Proactive);
}

#[test]
fn at_99_with_only_spent_peers_it_holds() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(99., &h, 109. * 3600.)),
        ("2", r(100., &h, 80. * 3600.)),
        ("3", r(100., &h, 50. * 3600.)),
    ];
    assert_ne!(h.tick_with_usage(&fleet), TickOutcome::Switched);
}

#[test]
fn below_the_brink_the_ordinary_rules_still_decide() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(50., &h, 109. * 3600.)),
        ("2", r(45., &h, 80. * 3600.)),
        ("3", r(40., &h, 50. * 3600.)),
    ];
    assert_ne!(h.tick_with_usage(&fleet), TickOutcome::Switched);
}

// -- TestReviewFindings202 --------------------------------------------------------------------

#[test]
fn a_short_interval_is_never_lengthened() {
    // max(min(delay, due_in), URGENT) raised a delay already below URGENT: a 15 s interval
    // slept 60 s.
    let mut h = Harness::standard();
    h.engine.active_plan = ActivePlan::Kept(h.now() + 5.);
    h.engine.settings.interval_seconds = 15.;
    let delay = h.engine.next_delay();
    assert!(delay <= 15. * 1.1, "a 15 s interval slept {delay:.1} s");
}

#[test]
fn recovery_reads_the_binding_windows_reset() {
    let h = Harness::standard();
    let entry = entry_for(
        &json!({"five_hour": {"pct": 40.0, "resets_at": at(&h, 3600.)},
                "seven_day": {"pct": 95.0}}),
        h.now(),
    );
    let reading = Reading::of(&entry, h.settings.threshold);
    assert!(reading.reset_key(h.now()).is_infinite());
}

#[test]
fn at_limit_still_ranks_by_headroom_when_all_are_above() {
    let mut h = Harness::standard();
    let fleet = [
        ("1", r(100., &h, 60.)),
        ("2", r(91., &h, 86400.)),
        ("3", r(97., &h, 120.)),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    assert_eq!(h.trigger(), Trigger::AtLimit);
}
