//! The meter on a real run loop: the test thread's own, run for one turn that does nothing but
//! wait, as a blocking read on the main thread would.

use std::cell::{Cell, RefCell};
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2_core_foundation::{
    CFAbsoluteTimeGetCurrent, CFRunLoop, CFRunLoopTimer, kCFRunLoopDefaultMode,
};

use super::*;

thread_local! {
    static STALLS: RefCell<Vec<Duration>> = const { RefCell::new(Vec::new()) };
    static WORK: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

fn told(ran: Duration) {
    STALLS.with_borrow_mut(|stalls| stalls.push(ran));
}

/// The stalls the meter tells of over one run of the thread's loop, with one turn that holds
/// the thread for `held`, then stops the loop.
fn stalls_over_a_turn_of(held: Duration) -> Vec<Duration> {
    WORK.set(held);
    let run_loop = CFRunLoop::current().unwrap();
    let observers = watch(&run_loop, told);
    let work = RcBlock::new(|_: *mut CFRunLoopTimer| {
        let (_open, never) = mpsc::channel::<()>();
        let _ = never.recv_timeout(WORK.get());
        CFRunLoop::current().unwrap().stop();
    });
    // SAFETY: the default allocator, and a block that's copied by the call and runs on this
    // thread's run loop, once.
    let timer = unsafe {
        CFRunLoopTimer::with_handler(None, CFAbsoluteTimeGetCurrent(), 0., 0, 0, Some(&work))
    }
    .unwrap();
    // SAFETY: a mode CoreFoundation makes once and keeps for the life of the process.
    let default = unsafe { kCFRunLoopDefaultMode };
    run_loop.add_timer(Some(&timer), default);
    CFRunLoop::run_in_mode(default, 5., false);
    for observer in &observers {
        observer.invalidate();
    }
    STALLS.take()
}

#[test]
fn a_turn_that_holds_the_thread_past_the_line_is_a_stall_of_its_length() {
    let held = STALL + Duration::from_millis(30);
    let stalls = stalls_over_a_turn_of(held);
    assert_eq!(stalls.len(), 1, "{stalls:?}");
    assert!(
        stalls[0] >= held && stalls[0] < held + Duration::from_secs(1),
        "{stalls:?}"
    );
}

#[test]
fn a_short_turn_is_no_stall() {
    assert_eq!(
        stalls_over_a_turn_of(Duration::from_millis(5)),
        Vec::<Duration>::new()
    );
}
