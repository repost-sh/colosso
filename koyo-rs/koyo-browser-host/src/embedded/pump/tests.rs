use std::ffi::c_void;
use std::sync::atomic::Ordering;

use super::{CFAbsoluteTimeGetCurrent, TIMER, arm, install, schedule};

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
