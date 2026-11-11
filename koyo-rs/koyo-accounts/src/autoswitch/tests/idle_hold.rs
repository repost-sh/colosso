//! cswap's `TestIdleHold`: the active token expired while Claude Code owns it, so hold, don't
//! fail over.

use serde_json::{Value, json};

use super::*;
use crate::oauth::USAGE_FOREIGN_CREDENTIAL;

#[test]
fn token_expired_holds_instead_of_failover() {
    let mut h = Harness::standard();
    for _ in 0..6 {
        // far past unhealthy_ticks (3)
        assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
        h.advance(60.);
    }
    assert_eq!(h.active(), Some(1));
    assert!(h.all::<SwitchEvent>().is_empty());
    let reasons: BTreeSet<&str> = h.reasons().into_iter().collect();
    assert_eq!(reasons, BTreeSet::from(["active-idle"]));
    assert_eq!(h.engine.unhealthy_ticks, 0);
}

#[test]
fn idle_hold_slows_cadence() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&held());
    assert_eq!(outcome, TickOutcome::NoAction);
    assert!(h.engine.next_delay() >= NO_RESET_FALLBACK_S);
}

#[test]
fn idle_hold_cap_escalates_to_failover() {
    let mut h = Harness::standard();
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
    h.advance(IDLE_HOLD_MAX_S + 1.);
    // Past the cap the sentinel counts as unhealthy again: failover after unhealthy_ticks (3).
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
}

#[test]
fn recovery_resets_the_hold_clock() {
    let mut h = Harness::standard();
    let healthy = [("1", usage(50.)), ("2", usage(10.)), ("3", usage(20.))];
    h.tick_with_usage(&held());
    h.advance(IDLE_HOLD_MAX_S - 60.);
    h.tick_with_usage(&healthy); // the person came back; the token refreshed
    h.advance(120.);
    // A new expiry long after: the hold clock restarted, so still held.
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
    assert_eq!(h.engine.unhealthy_ticks, 0);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn plain_fetch_failure_still_counts_unhealthy() {
    // An unknown reading (network failure, dead credentials) isn't the idle sentinel: unhealthy
    // counting and the hold clock's reset both apply.
    let mut h = Harness::standard();
    h.tick_with_usage(&held());
    let unknown = [("1", Value::Null), ("2", usage(10.)), ("3", usage(20.))];
    assert_eq!(h.tick_with_usage(&unknown), TickOutcome::NoAction);
    assert_eq!(h.engine.unhealthy_ticks, 1);
    assert_eq!(h.engine.idle_hold_since, None);
}

#[test]
fn foreign_credential_sentinel_fails_over_instead_of_holding() {
    // The foreign sentinel (the live credential is another account's) must not idle-hold like
    // an expired token: holding keeps the drift, while the failover switch repairs it.
    let mut h = Harness::standard();
    let foreign = [
        ("1", json!(USAGE_FOREIGN_CREDENTIAL)),
        ("2", usage(10.)),
        ("3", usage(20.)),
    ];
    assert_eq!(h.tick_with_usage(&foreign), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&foreign), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&foreign), TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
    assert_eq!(h.engine.idle_hold_since, None);
}
