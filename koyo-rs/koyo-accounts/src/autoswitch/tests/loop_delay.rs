//! cswap's `TestRunLoop` (its delays, and what of the loop is the engine's here: a tick that
//! fails), `TestLoopObeysThePollPlan` and `TestSessionThreshold`. The loop's `stop` and `wake`
//! and its sleep event are the app's, so their tests are left out.

use std::collections::BTreeSet;

use super::*;
use crate::poll_policy::{EXHAUSTED_INTERVAL_S, URGENT_INTERVAL_S};

#[test]
fn loop_survives_raising_tick() {
    let mut h = Harness::standard();
    h.host.lock().fail.entries = true;
    assert_eq!(h.tick(), TickOutcome::Error);
    assert!(!h.all::<ErrorEvent>().is_empty());
    h.host.lock().fail.entries = false;
    let outcome = h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(outcome, TickOutcome::NoAction);
}

#[test]
fn a_roster_failure_is_the_ticks_error() {
    let mut h = Harness::standard();
    h.host.lock().fail.roster = true;
    assert_eq!(h.tick(), TickOutcome::Error);
    let errors = h.all::<ErrorEvent>();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].message.contains("sequence unreadable"),
        "{errors:?}"
    );
    assert!(h.host.lock().fetches.is_empty(), "nothing collected");
}

#[test]
fn blocked_with_reset_rechecks_at_exhausted_cadence() {
    let mut h = Harness::standard();
    h.engine.wait = Wait::Until(h.now() + 1800.);
    assert_exact(h.engine.next_delay(), EXHAUSTED_INTERVAL_S);
}

#[test]
fn blocked_exhausted_without_reset_uses_fallback() {
    let mut h = Harness::standard();
    h.engine.wait = Wait::Long;
    assert_exact(h.engine.next_delay(), 300.);
}

#[test]
fn blocked_on_resolvable_condition_keeps_normal_cadence() {
    let mut h = Harness::standard();
    h.engine.wait = Wait::Interval;
    let delay = h.engine.next_delay();
    assert!((0.9 * 60. ..=1.1 * 60.).contains(&delay));
}

#[test]
fn normal_delay_is_jittered_interval() {
    let h = Harness::standard();
    for draw in [|| 0., || 0.5, || 0.999_999] {
        let mut engine = AutoSwitchEngine::new(&h.env, h.host.clone(), defaults());
        engine.rng = draw;
        let delay = engine.next_delay();
        assert!((0.9 * 60. ..=1.1 * 60.).contains(&delay), "{delay}");
    }
}

#[test]
fn sleep_cap() {
    let mut h = Harness::standard();
    h.engine.wait = Wait::Until(h.now() + 50. * 3600.);
    assert_exact(h.engine.next_delay(), EXHAUSTED_INTERVAL_S);
}

/// A tick under the line, the active row planned `due_in` seconds out.
fn tick_planned(h: &mut Harness, due_in: f64) -> TickOutcome {
    let now = h.now();
    let mut active = entry_for(&usage(50.), now);
    active.next_poll_at = Some(now + due_in);
    h.tick_with_entries(BTreeMap::from([
        ("1".to_owned(), active),
        ("2".to_owned(), entry_for(&usage(10.), now)),
        ("3".to_owned(), entry_for(&usage(10.), now)),
    ]))
}

#[test]
fn sleep_is_cut_to_the_rows_next_poll() {
    let mut h = Harness::standard();
    h.engine.settings.interval_seconds = 360.;
    assert_eq!(tick_planned(&mut h, 60.), TickOutcome::NoAction);
    assert_exact(h.delay, 60.);
}

#[test]
fn never_sleeps_below_the_planners_own_floor() {
    let mut h = Harness::standard();
    h.engine.settings.interval_seconds = 360.;
    tick_planned(&mut h, -500.);
    assert_exact(h.delay, URGENT_INTERVAL_S);
}

#[test]
fn a_relaxed_plan_never_lengthens_the_sleep() {
    let mut h = Harness::standard();
    tick_planned(&mut h, 3600.);
    assert!(h.delay <= 1.1 * 60.);
}

#[test]
fn a_switch_reads_the_new_active_rows_plan() {
    let mut h = Harness::standard();
    h.engine.settings.interval_seconds = 360.;
    let now = h.now();
    let mut target = entry_for(&usage(10.), now);
    target.next_poll_at = Some(now + 90.);
    let mut left = entry_for(&usage(95.), now);
    left.next_poll_at = Some(now + 3600.);
    let outcome = h.tick_with_entries(BTreeMap::from([
        ("1".to_owned(), left),
        ("2".to_owned(), target),
    ]));
    assert_eq!(outcome, TickOutcome::Switched);
    assert_exact(h.delay, 90.);
}

#[test]
fn a_store_failure_leaves_the_cadence_alone() {
    let mut h = Harness::standard();
    h.engine.active_plan = ActivePlan::Replanned;
    h.host.lock().fail.entries = true;
    let delay = h.engine.next_delay();
    assert!((0.9 * 60. ..=1.1 * 60.).contains(&delay));
}

#[test]
fn apply_threshold_retargets_trigger_and_poll_pin() {
    let mut h = Harness::standard();
    h.engine.set_threshold(72.);
    assert_exact(h.engine.settings.threshold, 72.);
    assert_eq!(h.host.lock().poll_thresholds.last(), Some(&72.));
    let outcome = h.tick_with_usage(&[("1", usage(80.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(outcome, TickOutcome::Switched);
}

#[test]
fn dropping_the_engine_unpins_the_poll_line() {
    let h = Harness::standard();
    let host = h.host.clone();
    assert!(!host.lock().poll_unpinned);
    drop(h);
    assert!(host.lock().poll_unpinned);
}

#[test]
fn a_threshold_is_clamped_and_pinned_once() {
    let mut h = Harness::standard();
    h.tick_with_usage(&[("1", usage(50.)), ("2", usage(10.)), ("3", usage(10.))]);
    assert_eq!(h.host.lock().poll_thresholds, [90.]);
    for (asked, kept) in [(20., 50.), (100., 99.9), (f64::NAN, 90.)] {
        h.engine.set_threshold(asked);
        assert_exact(h.engine.settings.threshold, kept);
        assert_exact(AutoSwitchSettings::new(asked).threshold, kept);
    }
}

fn collect_fetch_sets(h: &Harness, threshold: f64) -> Vec<BTreeSet<String>> {
    {
        let mut fake = h.host.lock();
        fake.fetches.clear();
        fake.entries = ["1", "2", "3"]
            .into_iter()
            .map(|n| {
                (
                    n.to_owned(),
                    entry_for(&usage(if n == "1" { 80. } else { 10. }), START),
                )
            })
            .collect();
    }
    let candidates = ["2".to_owned(), "3".to_owned()];
    h.engine
        .collect_scheduled_usage("1", &candidates, &[], threshold)
        .unwrap();
    h.host
        .lock()
        .fetches
        .iter()
        .map(|(fetch, _)| fetch.clone())
        .collect()
}

#[test]
fn collect_escalates_on_the_tick_snapshot_threshold() {
    let h = Harness::standard();
    let all: BTreeSet<String> = ["1", "2", "3"].into_iter().map(str::to_owned).collect();
    assert!(collect_fetch_sets(&h, 90.).contains(&all));
    assert!(!collect_fetch_sets(&h, 99.9).contains(&all));
}
