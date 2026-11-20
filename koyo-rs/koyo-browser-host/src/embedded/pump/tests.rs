use std::ffi::c_void;
use std::sync::atomic::Ordering;

use super::{CFAbsoluteTimeGetCurrent, Load, TIMER, arm, install, schedule};

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopTimerGetNextFireDate(timer: *mut c_void) -> f64;
}

/// When `timer` fires next.
fn next_fire(timer: *mut c_void) -> f64 {
    // SAFETY: a timer of the main run loop's, which keeps it.
    unsafe { CFRunLoopTimerGetNextFireDate(timer) }
}

/// The net armed as a turn ends is when the timer fires at the latest: an ask for later leaves
/// it, and only an ask for sooner brings the timer forward.
#[test]
fn an_ask_for_later_leaves_the_net_and_one_for_sooner_wins() {
    install();
    let timer = TIMER.load(Ordering::Acquire);
    assert!(!timer.is_null(), "the timer is made");
    // SAFETY: a CF call with no arguments.
    let net = unsafe { CFAbsoluteTimeGetCurrent() } + 1.;
    arm(timer, net);
    assert!((next_fire(timer) - net).abs() < 1e-3, "the net armed");
    schedule(5_000);
    let later = next_fire(timer) - net;
    assert!(later.abs() < 1e-3, "an ask for later leaves it: {later} s after it");
    schedule(0);
    let sooner = net - next_fire(timer);
    assert!(sooner > 0.5, "an ask for now wins: {sooner} s before it");
}

/// When the turns after one someone asked for run, with nobody asking again: seconds after it.
fn quiet_turns(load: Load, count: usize) -> Vec<f64> {
    let mut gap = load.net(0., true, false);
    let mut at = gap;
    let mut turns = vec![at];
    for _ in 1..count {
        gap = load.net(gap, false, false);
        at += gap;
        turns.push(at);
    }
    turns
}

/// With a page on screen the net is a thirtieth of a second after a turn someone asked for, and
/// each turn nobody asked for doubles it, up to a second, which it then keeps.
#[test]
#[expect(clippy::float_cmp, reason = "each is the constant itself, not near it")]
fn a_quiet_engine_runs_its_work_further_apart_up_to_a_second() {
    let shown = Load::Shown;
    assert_eq!(shown.net(1., true, false), 1. / 30., "asked for");
    assert_eq!(shown.net(1. / 30., false, false), 2. / 30., "nobody asked");
    assert_eq!(shown.net(0.8, false, false), 1., "up to a second");
    assert_eq!(shown.net(1., false, false), 1., "and no further");
    assert_eq!(shown.net(1., false, true), 1. / 30., "a turn that ran out of time");
    assert_eq!(Load::Unseen.net(0., true, false), 1. / 10., "pages, none on screen");
    assert_eq!(Load::Idle.net(0., true, false), 1., "nothing open");
}

/// While a person waits on the engine, or Chrome's own windows are open, its work runs every
/// thirtieth of a second whether or not anyone asks.
#[test]
#[expect(clippy::float_cmp, reason = "each is the constant itself, not near it")]
fn an_engine_waited_on_runs_its_work_every_thirtieth_of_a_second() {
    for gap in [0., 1. / 30., 1.] {
        assert_eq!(Load::Waited.net(gap, false, false), 1. / 30.);
    }
}

/// Work CEF never says it has, put off from a turn someone asked for, runs late by less than it
/// was put off for and a thirtieth of a second, and by a second at most.
#[test]
fn work_put_off_in_a_turn_runs_late_by_no_more_than_it_was_put_off_for() {
    let turns = quiet_turns(Load::Shown, 40);
    for put_off in [0.01, 0.05, 0.2, 0.5, 0.9, 2., 10.] {
        let ran = turns.iter().copied().find(|&at| at >= put_off).unwrap();
        let late = ran - put_off;
        assert!(
            late < put_off + 1. / 30. && late <= 1.,
            "put off {put_off} s, ran {late} s late"
        );
    }
    let ceiling = quiet_turns(Load::Shown, 40).windows(2).last().map(|w| w[1] - w[0]).unwrap();
    assert!((ceiling - 1.).abs() < 1e-9, "a quiet engine's turns a second apart: {ceiling}");
}

/// When work put off for `put_off` from within the turn at `from` runs: the first of `turns` at
/// or after it is due.
fn runs(turns: &[f64], from: f64, put_off: f64) -> f64 {
    turns.iter().copied().find(|&at| at >= from + put_off).unwrap()
}

/// Work put off from within a turn nobody asked for, as each link of a chain of Chromium's
/// timers is after the first, runs late by up to the net after that turn, which has grown: by
/// up to a second, however little it was put off for. CEF keeps the delay of work put off
/// within a turn to itself, so the pump can't tell such a turn from an empty one.
#[test]
fn a_chain_of_work_put_off_runs_late_by_up_to_the_net_after_the_turn_it_was_put_off_in() {
    let turns = quiet_turns(Load::Shown, 40);
    let net_after = |turn: f64| {
        let at = turns.iter().position(|&at| (at - turn).abs() < 1e-9).unwrap();
        turns[at + 1] - turns[at]
    };
    for (first, then) in [(0.5, 0.05), (0.2, 0.2), (0.05, 0.01), (2., 0.1), (0.9, 0.9)] {
        let ran = runs(&turns, 0., first);
        let next = runs(&turns, ran, then);
        let late = next - (ran + then);
        assert!(
            late <= net_after(ran) && late <= 1.,
            "put off {then} s from the turn at {ran} s, ran {late} s late"
        );
    }
    let ran = runs(&turns, 0., 0.5);
    let late = runs(&turns, ran, 0.05) - (ran + 0.05);
    assert!(late > 0.4, "a link put off 50 ms half a second on runs {late} s late");
}
