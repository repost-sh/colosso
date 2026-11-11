//! cswap's `TestAllAboveHeadroomDoesNotFlap`, first part: over the threshold the headroom
//! ranking needs its own anti-flap margin, and the no-return bar keeps a move one-way (measured
//! live: four switches in 35 minutes, each buying one point).

use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;

/// cswap's `_days_out`: an absolute reset anchored on first use per `hours`, so it draws nearer
/// as the clock advances.
#[derive(Default)]
pub(super) struct DaysOut(HashMap<u64, String>);

impl DaysOut {
    pub fn at(&mut self, h: &Harness, hours: f64) -> String {
        self.0
            .entry(hours.to_bits())
            .or_insert_with(|| iso_at(h.now() + hours * 3600.))
            .clone()
    }
}

/// The distinct consecutive actives of a trace.
pub(super) fn moves(seen: &[u32]) -> Vec<u32> {
    let mut moves: Vec<u32> = Vec::new();
    for &active in seen {
        if moves.last() != Some(&active) {
            moves.push(active);
        }
    }
    moves
}

#[test]
fn one_point_of_headroom_does_not_move() {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let fleet = [
        ("1", usage_r(95., &days.at(&h, 109.))),
        ("2", usage_r(94., &days.at(&h, 80.))),
        ("3", usage_r(99., &days.at(&h, 50.))),
    ];
    let outcome = h.tick_with_usage(&fleet);
    assert_eq!(h.active(), Some(1), "moved for one point of headroom");
    assert_ne!(outcome, TickOutcome::Switched);
}

#[test]
fn the_return_leg_is_blocked_too() {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let fleet = [
        ("1", usage_r(96., &days.at(&h, 109.))),
        ("2", usage_r(95., &days.at(&h, 80.))),
        ("3", usage_r(99., &days.at(&h, 50.))),
    ];
    let outcome = h.tick_with_usage(&fleet);
    assert_eq!(h.active(), Some(1));
    assert_ne!(outcome, TickOutcome::Switched);
}

#[test]
fn nine_points_beat_a_spent_peer_back_in_an_hour() {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let fleet = [
        ("1", usage_r(99., &days.at(&h, 300.))),
        ("2", usage_r(98.5, &days.at(&h, 1.))),
        ("3", usage_r(91., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
}

/// Burns the active each tick and returns the actives seen.
fn burn_walk(
    h: &mut Harness,
    mut pct: [f64; 2],
    resets: [f64; 2],
    ticks: usize,
    step: f64,
) -> Vec<u32> {
    let mut days = DaysOut::default();
    let mut seen = Vec::new();
    for _ in 0..ticks {
        let fleet = [
            ("1", usage_r(pct[0], &days.at(h, resets[0]))),
            ("2", usage_r(pct[1], &days.at(h, resets[1]))),
        ];
        h.tick_with_usage(&fleet);
        let active = h.active().unwrap();
        seen.push(active);
        let burning = &mut pct[active as usize - 1];
        *burning = (*burning + step).min(99.95);
        h.advance(301.);
    }
    seen
}

#[test]
fn a_burn_walk_settles_instead_of_oscillating() {
    // Only the active burns, both resets days out: at most two moves in 24 ticks, then still.
    let mut h = Harness::standard();
    let seen = burn_walk(&mut h, [96., 92.], [20., 80.], 24, 0.25);
    assert!(moves(&seen).len() <= 2, "move sequence {:?}", moves(&seen));
    assert!(
        seen[seen.len() - 4..]
            .iter()
            .all(|n| *n == seen[seen.len() - 1]),
        "{seen:?}"
    );
}

#[test]
fn the_no_return_filter_does_not_block_the_at_limit_escape() {
    // The bar is scoped to the proactive triggers: escaping a dead account, the account left
    // may be the only place to go (measured unscoped: 720 ticks of "no-candidates").
    let mut h = Harness::standard();
    h.update_state(&json!({"lastSwitchFrom": "2"}));
    let outcome = h.tick_with_usage(&[("1", usage(100.)), ("2", usage(0.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn a_burn_walk_never_returns_to_what_it_left() {
    // The walk is bounded, not return-free: it settles (60 ticks, the last eight still).
    let mut h = Harness::standard();
    let seen = burn_walk(&mut h, [92., 92.], [500., 400.], 60, 0.5);
    let tail: BTreeSet<u32> = seen[seen.len() - 8..].iter().copied().collect();
    assert_eq!(tail.len(), 1, "move sequence {:?}", moves(&seen));
}

#[test]
fn a_proactive_move_does_not_lock_out_the_next_one() {
    // On two accounts the bar removes the only candidate, and only a switch rewrites it: after
    // one ordinary move the peer resets to full while the active burns, and a move must happen.
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(92., &days.at(&h, 500.))),
        ("2", usage_r(10., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(301.);
    let mut outcomes = Vec::new();
    for _ in 0..20 {
        let fleet = [
            ("1", usage_r(0., &days.at(&h, 400.))),
            ("2", usage_r(97., &days.at(&h, 500.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(1801.);
    }
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

/// cswap's three-account fleet after one proactive move 1 → 2, with 3 spent.
fn moved_with_a_spent_third() -> (Harness, DaysOut) {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(92., &days.at(&h, 500.))),
        ("2", usage_r(10., &days.at(&h, 400.))),
        ("3", usage_r(100., &days.at(&h, 300.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    h.advance(301.);
    h.clear_events();
    (h, days)
}

#[test]
fn a_filtered_candidate_does_not_forge_an_all_exhausted_claim() {
    // A barred peer can't vanish from the census, or "all exhausted" is claimed with it at 0%.
    let (mut h, mut days) = moved_with_a_spent_third();
    let fleet = [
        ("1", usage_r(0., &days.at(&h, 400.))), // the one left: full
        ("2", usage_r(97., &days.at(&h, 500.))),
        ("3", usage_r(100., &days.at(&h, 300.))), // genuinely spent
    ];
    h.tick_with_usage(&fleet);
    assert!(h.all::<AllExhaustedEvent>().is_empty(), "{:?}", h.kinds());
}

#[test]
fn the_bar_does_not_hide_the_account_from_the_census() {
    // The barred branch itself: 15 points against an active on 10 doesn't release.
    let (mut h, mut days) = moved_with_a_spent_third();
    let fleet = [
        ("1", usage_r(85., &days.at(&h, 400.))), // left; 15 points, barred
        ("2", usage_r(90., &days.at(&h, 500.))), // active; 10 points
        ("3", usage_r(100., &days.at(&h, 300.))),
    ];
    h.tick_with_usage(&fleet);
    assert!(h.all::<AllExhaustedEvent>().is_empty(), "{:?}", h.kinds());
}

#[test]
fn the_bar_never_applies_to_an_escape() {
    // 12 points against an active on 10 doesn't clear the capped margin (13), so only the
    // trigger check can answer.
    let state = json!({"lastSwitchFrom": 1});
    let headroom = [("1", Some(12.)), ("2", Some(10.)), ("3", Some(1.))];
    let usage: Vec<(&str, Value)> = headroom
        .iter()
        .map(|(n, h)| (*n, usage(100. - h.unwrap())))
        .collect();
    for trigger in [Trigger::AtLimit, Trigger::Failover] {
        for active in [Some(10.), None] {
            let direct = Direct::new(&usage, &headroom, active, "2");
            assert_eq!(
                direct.barred(trigger, &state, true),
                None,
                "{trigger:?} {active:?}"
            );
        }
    }
    // The control: the same state bars on a proactive tick.
    let direct = Direct::new(&usage, &headroom, Some(10.), "2");
    assert_eq!(
        direct.barred(Trigger::Proactive, &state, true).as_deref(),
        Some("1")
    );
}
