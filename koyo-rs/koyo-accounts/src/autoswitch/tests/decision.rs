//! cswap's `TestDecisionTable`.

use serde_json::{Value, json};

use super::*;

#[test]
fn below_threshold_is_no_action() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.active(), Some(1));
    assert_eq!(h.reasons(), ["below-threshold"]);
}

#[test]
fn over_threshold_switches_to_max_headroom() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(40.)), ("3", usage(20.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(3));
    let switch = h.all::<SwitchEvent>().remove(0);
    assert_eq!(switch.trigger, Trigger::Proactive);
    assert_eq!(
        switch.to_ref,
        Some(AccountRef {
            number: Some(3),
            email: "c@example.com".into()
        })
    );
    assert_eq!(h.state()["lastSwitchTo"], "3");
}

#[test]
fn no_active_account() {
    let mut h = Harness::new(defaults());
    assert_eq!(h.tick(), TickOutcome::NoAction);
    assert_eq!(h.reasons(), ["no-active-account"]);
}

#[test]
fn hysteresis_margin_blocks_marginal_candidates() {
    // threshold 90, hysteresis 10: a candidate must beat the active by >= 10 points; 95 → 86
    // is only 9. Failing the margin isn't exhaustion: no all-exhausted, no reset sleep, so the
    // next tick keeps the normal cadence and the at-limit escape isn't missed.
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(86.)), ("3", usage(88.))]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
    assert!(h.all::<AllExhaustedEvent>().is_empty());
    assert_eq!(h.reasons(), ["no-qualifying-candidate"]);
    assert_eq!(h.engine.wait, Wait::Interval);
    assert!(h.engine.next_delay() <= 1.1 * h.settings.interval_seconds);
}

#[test]
fn issue_115_strictly_better_candidate_switches() {
    // Active bound by 5h (99%), candidate by 7d (89%): 89 < 90 and 99 - 89 >= 10.
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        (
            "1",
            json!({"five_hour": {"pct": 99.0}, "seven_day": {"pct": 24.0}}),
        ),
        (
            "2",
            json!({"five_hour": {"pct": 3.0}, "seven_day": {"pct": 89.0}}),
        ),
        (
            "3",
            json!({"five_hour": {"pct": 95.0}, "seven_day": {"pct": 10.0}}),
        ),
    ]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Proactive);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn proactive_never_lands_at_or_over_threshold() {
    // threshold 80, hysteresis 15: 85% is five points better than the active 90% but over the
    // line; the 79% peer keeps this out of the all-above escape and misses the margin itself.
    let settings = AutoSwitchSettings {
        hysteresis_pct: 15.,
        ..threshold(80.)
    };
    let mut h = Harness::fleet(3, settings);
    let outcome = h.tick_with_usage(&[("1", usage(90.)), ("2", usage(85.)), ("3", usage(79.))]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
    assert_eq!(h.reasons(), ["no-qualifying-candidate"]);
}

#[test]
fn stable_landing_does_not_switch_back() {
    // Cooldown off so only the gate prevents flapping: after 99 → 89 the old account (99%) can
    // never beat the new active (89%).
    let settings = AutoSwitchSettings {
        cooldown_seconds: 0.,
        ..defaults()
    };
    let mut h = Harness::fleet(2, settings);
    let fleet = [
        (
            "1",
            json!({"five_hour": {"pct": 99.0}, "seven_day": {"pct": 24.0}}),
        ),
        (
            "2",
            json!({"five_hour": {"pct": 3.0}, "seven_day": {"pct": 89.0}}),
        ),
    ];
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
    h.advance(60.);
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::NoAction);
    assert_eq!(h.active(), Some(2));
    assert_eq!(h.reasons(), ["below-threshold"]);
}

#[test]
fn mixed_unknown_and_exhausted_is_not_all_exhausted() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        ("1", usage(95.)),
        ("2", usage_r(100., "2026-07-03T12:00:00Z")),
        ("3", Value::Null),
    ]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert!(h.all::<AllExhaustedEvent>().is_empty());
    assert_eq!(h.reasons(), ["no-qualifying-candidate"]);
    assert_eq!(h.engine.wait, Wait::Interval);
    assert!(h.engine.next_delay() <= 1.1 * h.settings.interval_seconds);
}

fn stale(value: Value, now: f64, failures: i64, trust_extended: bool) -> UsageEntry {
    UsageEntry {
        last_good: Some(value),
        fetched_at: Some(now - 400.),
        age_s: Some(400.),
        consecutive_failures: failures,
        trust_extended,
        ..UsageEntry::default()
    }
}

#[test]
fn stale_beyond_trust_blocks_all_exhausted() {
    let mut h = Harness::standard();
    let now = h.now();
    let reset = "2026-07-05T12:00:00Z";
    let outcome = h.tick_with_entries(BTreeMap::from([
        ("1".to_owned(), entry_for(&usage(95.), now)),
        ("2".to_owned(), stale(usage_r(100., reset), now, 1, true)),
        ("3".to_owned(), stale(usage(10.), now, 0, false)),
    ]));
    assert_eq!(outcome, TickOutcome::Blocked);
    assert!(h.all::<AllExhaustedEvent>().is_empty());
    assert_eq!(h.reasons(), ["no-qualifying-candidate"]);
}

#[test]
fn trusted_stale_exhausted_set_still_fires_all_exhausted() {
    let mut h = Harness::standard();
    let now = h.now();
    let reset = "2026-07-05T12:00:00Z";
    let exhausted = stale(usage_r(100., reset), now, 1, true);
    let outcome = h.tick_with_entries(BTreeMap::from([
        ("1".to_owned(), entry_for(&usage(95.), now)),
        ("2".to_owned(), exhausted.clone()),
        ("3".to_owned(), exhausted),
    ]));
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(
        h.all::<AllExhaustedEvent>()
            .first()
            .unwrap()
            .earliest_reset_at
            .map(iso)
            .as_deref(),
        Some(reset)
    );
}

#[test]
fn cooldown_suppresses_proactive() {
    let mut h = Harness::standard();
    h.update_state(&json!({"lastSwitchAt": h.now() - 10.}));
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.reasons(), ["cooldown"]);
}

#[test]
fn at_limit_bypasses_cooldown() {
    let mut h = Harness::standard();
    h.update_state(&json!({"lastSwitchAt": h.now() - 10.}));
    let outcome = h.tick_with_usage(&[("1", usage(100.)), ("2", usage(10.)), ("3", usage(50.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::AtLimit);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn cooldown_expires() {
    let mut h = Harness::standard();
    h.update_state(&json!({"lastSwitchAt": h.now()}));
    h.advance(400.); // past the 300 s default
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.)), ("3", usage(50.))]);
    assert_eq!(outcome, TickOutcome::Switched);
}

fn unknown_active() -> [(&'static str, Value); 3] {
    [("1", Value::Null), ("2", usage(10.)), ("3", usage(50.))]
}

#[test]
fn unknown_active_usage_waits_then_fails_over() {
    let mut h = Harness::standard();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn unhealthy_count_survives_a_fresh_engine_per_tick() {
    let mut h = Harness::standard();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    assert_eq!(h.state()["unhealthyTicks"], 1);
    h.renew();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    h.renew();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

fn idle_in_last_two(h: &Harness) -> bool {
    let events = h.events();
    events[events.len().saturating_sub(2)..]
        .iter()
        .any(|event| matches!(event, AutoSwitchEvent::NoSwitch(e) if e.reason == NoSwitchReason::ActiveIdle))
}

#[test]
fn idle_hold_clock_survives_a_fresh_engine_per_tick() {
    let mut h = Harness::standard();
    h.tick_with_usage(&held());
    h.advance(IDLE_HOLD_MAX_S + 1.);
    h.renew();
    assert_eq!(h.tick_with_usage(&held()), TickOutcome::NoAction);
    assert_eq!(h.state()["unhealthyTicks"], 1);
    assert!(!idle_in_last_two(&h));
}

#[test]
fn persisted_count_ignored_for_another_account_or_stale_tick() {
    let mut h = Harness::standard();
    h.tick_with_usage(&unknown_active());
    h.tick_with_usage(&unknown_active());
    assert_eq!(h.state()["unhealthyTicks"], 2);
    h.advance(2. * IDLE_HOLD_MAX_S + 1.);
    h.renew();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    assert_eq!(h.state()["unhealthyTicks"], 1);
    h.update_state(&json!({"healthAccount": "2"}));
    h.renew();
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::NoAction);
    assert_eq!(h.state()["unhealthyTicks"], 1);
}

#[test]
fn count_built_on_one_account_is_not_saved_under_the_next() {
    let mut h = Harness::standard();
    let unknown = [("1", Value::Null), ("2", Value::Null), ("3", usage(50.))];
    h.tick_with_usage(&unknown);
    h.tick_with_usage(&unknown);
    assert_eq!(h.state()["unhealthyTicks"], 2);
    // The same engine, the active account changed from 1 to 2 (by hand).
    h.make_live(2);
    assert_eq!(h.tick_with_usage(&unknown), TickOutcome::NoAction);
    let state = h.state();
    assert_eq!(state["healthAccount"], "2");
    assert_eq!(state["unhealthyTicks"], 1);
    h.renew();
    assert_eq!(h.tick_with_usage(&unknown), TickOutcome::NoAction);
    assert_eq!(h.state()["unhealthyTicks"], 2);
}

#[test]
fn no_active_tick_keeps_the_health_context() {
    let mut h = Harness::standard();
    h.tick_with_usage(&unknown_active());
    h.tick_with_usage(&unknown_active());
    h.host.lock().hide_current = true;
    h.tick_with_usage(&unknown_active());
    h.host.lock().hide_current = false;
    assert_eq!(h.engine.health_account.as_deref(), Some("1"));
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::Switched);
}

#[test]
fn loop_progress_survives_a_long_gap_between_ticks() {
    let mut h = Harness::standard();
    h.tick_with_usage(&unknown_active());
    h.tick_with_usage(&unknown_active());
    h.advance(3. * IDLE_HOLD_MAX_S);
    assert_eq!(h.tick_with_usage(&unknown_active()), TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn healthy_reading_clears_persisted_counters() {
    let mut h = Harness::standard();
    h.tick_with_usage(&[("1", Value::Null), ("2", usage(10.)), ("3", usage(10.))]);
    h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))]);
    let state = h.state();
    assert!(state.get("unhealthyTicks").is_none());
    assert!(state.get("idleHoldSince").is_none());
}

#[test]
fn known_active_usage_resets_unhealthy_counter() {
    let mut h = Harness::standard();
    let unknown = [("1", Value::Null), ("2", usage(10.)), ("3", usage(10.))];
    let healthy = [("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))];
    h.tick_with_usage(&unknown);
    h.tick_with_usage(&unknown);
    h.tick_with_usage(&healthy); // resets the counter
    assert_eq!(h.tick_with_usage(&unknown), TickOutcome::NoAction);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn all_candidates_unknown_is_no_comparison() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", Value::Null), ("3", Value::Null)]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.reasons(), ["no-comparison"]);
}

#[test]
fn tie_resolves_to_earliest_slot() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(30.)), ("3", usage(30.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn candidate_not_better_than_active_is_skipped() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(91.)), ("2", usage(95.)), ("3", usage(99.))]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn at_limit_escapes_hysteresis_bar() {
    // Active hard at 100%; the only room is a candidate at 85%, which the proactive margin
    // would refuse. At-limit is an escape: any account with real headroom beats a blocked one.
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(100.)), ("2", usage(85.)), ("3", usage(97.))]);
    assert_eq!(outcome, TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::AtLimit);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn at_limit_never_targets_another_at_limit_account() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[("1", usage(100.)), ("2", usage(100.)), ("3", usage(100.))]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(h.active(), Some(1));
}

#[test]
fn failover_ignores_hysteresis_bar() {
    let mut h = Harness::standard();
    let fleet = [("1", Value::Null), ("2", usage(85.)), ("3", usage(100.))];
    h.tick_with_usage(&fleet);
    h.tick_with_usage(&fleet);
    assert_eq!(h.tick_with_usage(&fleet), TickOutcome::Switched);
    assert_eq!(h.trigger(), Trigger::Failover);
    assert_eq!(h.active(), Some(2));
}

#[test]
fn unmanaged_live_login_is_never_touched() {
    let mut h = Harness::new(defaults());
    h.seed(1, "a@example.com");
    h.seed(2, "b@example.com");
    // A login Koyo doesn't manage.
    h.host.lock().unmanaged_live = true;
    let outcome = h.tick_with_usage(&[("1", usage(95.)), ("2", usage(10.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
    assert_eq!(h.reasons(), ["unmanaged-active-account"]);
    assert!(h.host.lock().switch_log.is_empty());
}

#[test]
fn all_exhausted_carries_earliest_reset() {
    let mut h = Harness::standard();
    let outcome = h.tick_with_usage(&[
        ("1", usage_r(100., "2026-07-03T12:00:00Z")),
        ("2", usage_r(100., "2026-07-03T10:30:00Z")),
        ("3", usage_r(100., "2026-07-03T11:00:00Z")),
    ]);
    assert_eq!(outcome, TickOutcome::Blocked);
    assert_eq!(
        h.all::<AllExhaustedEvent>()
            .first()
            .unwrap()
            .earliest_reset_at
            .map(iso)
            .as_deref(),
        Some("2026-07-03T10:30:00Z")
    );
    assert!(matches!(h.engine.wait, Wait::Until(_)));
}

#[test]
fn all_exhausted_ignores_non_future_reset() {
    for offset in [-60., 0.] {
        let mut h = Harness::standard();
        let reset = iso(h.now() + offset);
        let outcome = h.tick_with_usage(&[
            ("1", usage_r(100., &reset)),
            ("2", usage_r(100., &reset)),
            ("3", usage_r(100., &reset)),
        ]);
        assert_eq!(outcome, TickOutcome::Blocked);
        assert_eq!(
            h.all::<AllExhaustedEvent>()
                .first()
                .unwrap()
                .earliest_reset_at,
            None
        );
        assert_eq!(h.engine.wait, Wait::Long);
        assert_exact(h.engine.next_delay(), NO_RESET_FALLBACK_S);
    }
}
