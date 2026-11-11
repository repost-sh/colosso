//! The no-return release predicate called directly, one state shape a test: cswap's
//! `TestTheReleasePredicateOneStateShapePerTest`, the direct cases of
//! `TestAllAboveHeadroomDoesNotFlap` and of `TestConsumeFirstDepartureRecordsItsOwnTrigger` (a
//! state file's `leftTrigger` decides which legs run, whatever strategy wrote it). A failover
//! record carries its `leftTrigger`, as every one Koyo writes does; cswap's cases of records
//! written before a field existed are left out with that reading.

use serde_json::{Value, json};

use super::*;

/// `_left_account_recovered` on the default settings, no current account unless given.
fn recovered_on(
    settings: &AutoSwitchSettings,
    state: &Value,
    usage: &[(&str, Value)],
    headroom: &[(&str, Option<f64>)],
    active: Option<f64>,
    current: &str,
) -> bool {
    Direct {
        settings: settings.clone(),
        ..Direct::new(usage, headroom, active, current)
    }
    .recovered(state)
}

fn recovered(
    state: &Value,
    usage: &[(&str, Value)],
    headroom: &[(&str, Option<f64>)],
    active: Option<f64>,
) -> bool {
    recovered_on(&defaults(), state, usage, headroom, active, "")
}

fn null_snapshot(trigger: &str) -> Value {
    json!({"lastSwitchFrom": "1", "leftHeadroom": null, "leftRecoveryAt": null,
           "leftTrigger": trigger})
}

// -- TestConsumeFirstDepartureRecordsItsOwnTrigger ---------------------------------------------

#[test]
fn the_null_snapshot_answers_differently_by_recorded_trigger() {
    // Peer 11 points, active 5. Failover's landing floor (h > 10) releases; an ordinary
    // departure's dominance (h > 5 x 2 + 3 = 13) doesn't, and has no baseline.
    let usage = [("1", usage(89.)), ("2", usage(95.))];
    let headroom = [("1", Some(11.))];
    let settings = defaults();
    assert!(recovered_on(
        &settings,
        &null_snapshot("failover"),
        &usage,
        &headroom,
        Some(5.),
        "2"
    ));
    assert!(!recovered_on(
        &settings,
        &null_snapshot("consume-first"),
        &usage,
        &headroom,
        Some(5.),
        "2"
    ));
}

// -- TestAllAboveHeadroomDoesNotFlap, direct --------------------------------------------------

// The premise: unclamped, the headroom leg is unsatisfiable at 100.
const _: () = assert!(100. < 98. + SPENT_HEADROOM_PCT);

#[test]
fn the_clamp_stays_load_bearing_when_dominance_does_not_fire() {
    let state = json!({"lastSwitchFrom": "1", "leftHeadroom": 98.0, "leftRecoveryAt": null});
    assert!(recovered(
        &state,
        &[("1", usage(0.))],
        &[("1", Some(100.))],
        Some(60.)
    ));
}

#[test]
fn the_dominance_leg_does_not_silently_read_an_unreadable_active_as_no_dominance() {
    let state = json!({"lastSwitchFrom": "2", "leftHeadroom": 40.0, "leftRecoveryAt": null});
    let usage = [("2", usage(60.))]; // the peer frozen at 40 points
    let settings = defaults();
    let readable = recovered_on(
        &settings,
        &state,
        &usage,
        &[("2", Some(40.))],
        Some(2.),
        "1",
    );
    let unreadable = recovered_on(&settings, &state, &usage, &[("2", Some(40.))], None, "1");
    assert!(readable, "premise: a readable, dominated active releases");
    assert!(unreadable, "an unreadable active isn't 'not dominated'");
}

fn barred(
    state: &Value,
    headroom: &[(&str, Option<f64>)],
    active: Option<f64>,
    trigger: Trigger,
    recovered: bool,
) -> Option<String> {
    let usage: Vec<(&str, Value)> = headroom
        .iter()
        .map(|(n, h)| (*n, usage(100. - h.unwrap())))
        .collect();
    Direct::new(&usage, headroom, active, "").barred(trigger, state, recovered)
}

#[test]
fn no_return_account_does_not_re_bar_an_already_recovered_peer_when_the_active_is_unreadable() {
    let state = json!({"lastSwitchFrom": "2", "leftHeadroom": 40.0, "leftRecoveryAt": null});
    let headroom = [("2", Some(40.))];
    assert_eq!(
        barred(&state, &headroom, Some(2.), Trigger::Proactive, true),
        None
    );
    assert_eq!(
        barred(&state, &headroom, None, Trigger::Proactive, true),
        None
    );
}

#[test]
fn the_all_spent_recovery_leg_carries_its_own_hysteresis() {
    // The peer resets only 60 s before the active: inside RECOVERY_HYSTERESIS_S.
    let state = null_snapshot("failover");
    let usage = [
        ("1", usage_r(95., &iso(START + 3600.))),
        ("2", usage_r(98., &iso(START + 3660.))),
    ];
    let settings = defaults();
    assert!(!recovered_on(
        &settings,
        &state,
        &usage,
        &[("1", Some(5.))],
        Some(2.),
        "2"
    ));
}

#[test]
fn the_dominance_fallback_does_not_fire_below_its_own_floor() {
    let state = json!({"lastSwitchFrom": "2", "leftHeadroom": 40.0, "leftRecoveryAt": null});
    let settings = defaults();
    assert!(!recovered_on(
        &settings,
        &state,
        &[("2", usage(95.))],
        &[("2", Some(5.))],
        None,
        "1"
    ));
}

#[test]
fn no_return_accounts_unreadable_active_fallback_has_a_floor_too() {
    let state = json!({"lastSwitchFrom": "2"});
    assert_eq!(
        barred(&state, &[("2", Some(5.))], None, Trigger::Proactive, true).as_deref(),
        Some("2")
    );
}

// -- TestTheReleasePredicateOneStateShapePerTest ----------------------------------------------

fn failover_from_two() -> Value {
    json!({"lastSwitchFrom": "2", "leftHeadroom": null, "leftRecoveryAt": null,
           "leftTrigger": "failover"})
}

#[test]
fn failover_peer_readable_dominant_and_changed_releases() {
    assert!(recovered(
        &failover_from_two(),
        &[("2", usage(85.))],
        &[("2", Some(15.))],
        Some(2.)
    ));
}

#[test]
fn the_failover_floor_moves_with_the_users_threshold() {
    // threshold 60: 35 points is under its line's floor (40) and holds; threshold 71 (floor
    // 29): the same peer releases.
    let usage = [("2", usage(65.))];
    let headroom = [("2", Some(35.))];
    assert!(!recovered_on(
        &threshold(60.),
        &failover_from_two(),
        &usage,
        &headroom,
        Some(2.),
        ""
    ));
    assert!(recovered_on(
        &threshold(71.),
        &failover_from_two(),
        &usage,
        &headroom,
        Some(2.),
        ""
    ));
}

#[test]
fn failover_peer_still_poor_holds() {
    assert!(!recovered(
        &failover_from_two(),
        &[("2", usage(95.))],
        &[("2", Some(5.))],
        Some(2.)
    ));
}

#[test]
fn failover_peer_still_unreadable_holds() {
    assert!(!recovered(
        &failover_from_two(),
        &[("2", Value::Null)],
        &[("2", None)],
        Some(2.)
    ));
}

#[test]
fn ordinary_departure_weekly_bound_peer_that_recovered_releases() {
    // The reset moved from an hour out to a minute out: a recovery the headroom leg can't see.
    let state =
        json!({"lastSwitchFrom": "2", "leftHeadroom": 4.0, "leftRecoveryAt": START + 3600.});
    let usage = [("2", usage_r(96., &iso(START + 60.)))];
    assert!(recovered(&state, &usage, &[("2", Some(4.))], Some(8.)));
}

#[test]
fn at_limit_escape_still_works() {
    let state = json!({"lastSwitchFrom": "2"});
    let headroom = [("1", Some(0.)), ("2", Some(100.))];
    for active in [Some(0.), None] {
        for recovered in [true, false] {
            assert_eq!(
                barred(&state, &headroom, active, Trigger::AtLimit, recovered),
                None
            );
        }
    }
}
