//! cswap's `TestAllAboveHeadroomDoesNotFlap`, second part: where the no-return bar holds and
//! where it lifts (the engine's own landing, an empty ranking, a recovered account).

use serde_json::{Value, json};

use super::super::state::StoredSlot;
use super::no_return::DaysOut;
use super::*;

#[test]
fn the_bar_only_holds_while_the_engine_is_where_it_landed() {
    // A hand switch away from the landing undoes the move the bar guards. `lastSwitchTo` is
    // written as a string and `lastSwitchFrom` as a number; an absent `lastSwitchTo` keeps the
    // bar.
    for (landed, live, expect) in [
        (Some(json!("2")), 2, 3),
        (Some(json!("2")), 3, 1),
        (Some(json!(2)), 2, 3),
        (Some(json!(2)), 3, 1),
        (None, 3, 3),
    ] {
        let landed: Option<StoredSlot> =
            landed.map(|landed| serde_json::from_value(landed).unwrap());
        let mut h = Harness::fleet(3, threshold(80.));
        let back = [at(&h, 1800.), at(&h, 7200.), at(&h, 3600.)];
        let first = [
            ("1", usage_r(89., &back[0])),
            ("2", usage_r(10., &back[1])),
            ("3", usage_r(89., &back[2])),
        ];
        assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
        assert_eq!(h.active(), Some(2));
        let state = h.state();
        assert_eq!(
            (
                state["lastSwitchFrom"].clone(),
                state["lastSwitchTo"].clone()
            ),
            (json!(1), json!("2"))
        );
        h.edit_state(|state| state.last_switch_to.clone_from(&landed));
        if live != 2 {
            h.make_live(3); // the person switches by hand
        }
        h.advance(301.);
        let second = [
            ("1", usage_r(88., &back[0])),
            ("2", usage_r(95., &back[1])),
            ("3", usage_r(91.5, &back[2])),
        ];
        h.tick_with_usage(&second);
        assert_eq!(
            h.active(),
            Some(expect),
            "lastSwitchTo={landed:?} live={live}"
        );
    }
}

#[test]
fn the_bar_actually_removes_the_account_from_the_ranking() {
    // `_rank_candidates` called directly, as cswap's tests do.
    let usage = [("1", usage(40.)), ("2", usage(96.)), ("3", usage(99.))];
    let headroom = [("1", Some(60.)), ("2", Some(4.)), ("3", Some(1.))];
    let direct = Direct {
        candidates: &["1", "3"],
        ..Direct::new(&usage, &headroom, Some(4.), "2")
    };
    let rank = |no_return| direct.ranked(no_return);
    assert_eq!(
        rank(None),
        ["1"],
        "premise: 1 is the pick when nothing bars it"
    );
    assert!(
        rank(Some("1")).is_empty(),
        "the bar didn't remove account 1"
    );
}

/// One ordinary proactive move 1 → 2 on three accounts, 3 at `third`.
fn moved(third: f64) -> (Harness, DaysOut) {
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(92., &days.at(&h, 500.))),
        ("2", usage_r(10., &days.at(&h, 400.))),
        ("3", usage_r(third, &days.at(&h, 300.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    h.advance(301.);
    (h, days)
}

fn stalled_fleet(h: &Harness, days: &mut DaysOut, third: f64) -> [(&'static str, Value); 3] {
    [
        ("1", usage_r(92.5, &days.at(h, 10.))), // barred, 7.5 points, recovered (10 h out)
        ("2", usage_r(96., &days.at(h, 500.))), // active, 4 points
        ("3", usage_r(third, &days.at(h, 300.))),
    ]
}

#[test]
fn the_bar_lifts_when_the_only_alternative_cannot_be_chosen() {
    // A third account that exists but can't qualify (at its limit) offers the ranking nothing.
    let (mut h, mut days) = moved(100.);
    let mut outcomes = Vec::new();
    for _ in 0..30 {
        let fleet = stalled_fleet(&h, &mut days, 100.);
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(3601.);
    }
    assert!(
        outcomes.contains(&TickOutcome::Switched),
        "{:?}",
        &outcomes[..6]
    );
}

#[test]
fn the_bar_lifts_for_an_alternative_the_ranking_would_reject() {
    // A third account holding one point passes `> 0` and no ranking gate.
    let (mut h, mut days) = moved(99.);
    let mut outcomes = Vec::new();
    for _ in 0..30 {
        let fleet = stalled_fleet(&h, &mut days, 99.);
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(3601.);
    }
    assert!(
        outcomes.contains(&TickOutcome::Switched),
        "{:?}",
        &outcomes[..6]
    );
}

#[test]
fn an_unreadable_barred_account_does_not_crash_the_tick() {
    // Unknown headroom isn't evidence the account left beats us: it still bars.
    let headroom = [("2", Some(10.)), ("3", Some(40.))]; // slot 1 unreadable: absent
    let usage: Vec<(&str, Value)> = headroom
        .iter()
        .map(|(n, h)| (*n, usage(100. - h.unwrap())))
        .collect();
    let direct = Direct::new(&usage, &headroom, Some(10.), "2");
    assert_eq!(
        direct
            .barred(Trigger::Proactive, &json!({"lastSwitchFrom": 1}), true)
            .as_deref(),
        Some("1")
    );
}

#[test]
fn the_same_fleet_moves_with_the_bar_cleared() {
    let (mut h, mut days) = moved(99.);
    h.edit_state(|state| state.last_switch_from = None);
    let fleet = stalled_fleet(&h, &mut days, 99.);
    assert_eq!(
        h.tick_with_usage(&fleet),
        TickOutcome::Switched,
        "the control blocked too"
    );
}

#[test]
fn the_bar_reaches_the_ranking_through_tick() {
    // The bar's wiring: with the active above SPENT_HEADROOM_PCT the account left clears the
    // capped margin without having recovered, which is where the bar bites; the engine lands
    // elsewhere.
    let mut h = Harness::standard();
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(92., &days.at(&h, 500.))),
        ("2", usage_r(10., &days.at(&h, 400.))),
        ("3", usage_r(50., &days.at(&h, 300.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(301.);
    assert!(
        h.state().get("lastSwitchFrom").is_some(),
        "premise: the move recorded what it left"
    );
    let second = [
        ("1", usage_r(91., &days.at(&h, 500.))),  // left; 9 points
        ("2", usage_r(96., &days.at(&h, 400.))),  // active, 4 points
        ("3", usage_r(92.5, &days.at(&h, 300.))), // 7.5 points
    ];
    assert_eq!(h.tick_with_usage(&second), TickOutcome::Switched);
    assert_eq!(h.active(), Some(3), "the bar never reached the ranking");
    h.make_live(2);
    h.advance(301.);
    h.edit_state(|state| state.last_switch_from = None);
    assert_eq!(h.tick_with_usage(&second), TickOutcome::Switched);
    assert_eq!(
        h.active(),
        Some(1),
        "premise: unbarred, the account left is the pick"
    );
}

/// Two accounts: 1 left at 4 points (500 h out) for 2 at 8 (400 h out).
fn left_at_four() -> (Harness, DaysOut) {
    let mut h = Harness::fleet(2, defaults());
    let mut days = DaysOut::default();
    let first = [
        ("1", usage_r(96., &days.at(&h, 500.))),
        ("2", usage_r(92., &days.at(&h, 400.))),
    ];
    assert_eq!(h.tick_with_usage(&first), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(3612.);
    (h, days)
}

#[test]
fn the_release_needs_the_barred_account_to_have_improved() {
    // The account left is exactly as it was: the ranking flipped only because the active
    // burned, walked past the bare ratio's old boundary (active 2.0 → 0.5).
    let (mut h, mut days) = left_at_four();
    let mut outcomes = Vec::new();
    for active in [98., 98.2, 98.4, 99., 99.5] {
        let fleet = [
            ("1", usage_r(96., &days.at(&h, 500.))),
            ("2", usage_r(active, &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(!outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}

#[test]
fn the_release_fires_on_this_same_fleet_once_the_peer_actually_improves() {
    let (mut h, mut days) = left_at_four();
    let mut outcomes = Vec::new();
    for _ in 0..10 {
        let fleet = [
            ("1", usage_r(0., &days.at(&h, 500.))),
            ("2", usage_r(98., &days.at(&h, 400.))),
        ];
        outcomes.push(h.tick_with_usage(&fleet));
        h.advance(301.);
    }
    assert!(outcomes.contains(&TickOutcome::Switched), "{outcomes:?}");
}
