//! The main thread's stalls: a turn of its run loop long enough to cost three frames, logged
//! with how long it ran, in every build. Two run-loop observers time each turn, from the loop
//! waking to its going back to sleep, so the meter rides the turns that happen anyway and wakes
//! nothing. It says when and how long, not what: `script/perf-sample <pid> <s> <label> --profile`
//! over the same scene names the frames.
//!
//! A nested run loop, a menu's or a drag's, ends the turn that ran it when it first waits, so
//! the time a person spends in a menu is never a stall.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2_core_foundation::{
    CFRetained, CFRunLoop, CFRunLoopActivity, CFRunLoopObserver, kCFRunLoopCommonModes,
};

/// A turn this long or longer is a stall: three frames at 60 Hz.
pub const STALL: Duration = Duration::from_millis(50);

/// The run-loop activities a turn starts on and ends on.
const STARTS: CFRunLoopActivity = CFRunLoopActivity::Entry.union(CFRunLoopActivity::AfterWaiting);
const ENDS: CFRunLoopActivity = CFRunLoopActivity::BeforeWaiting.union(CFRunLoopActivity::Exit);

/// One run loop's turn under way, and where a stall is told.
struct Turns {
    started: Cell<Option<Instant>>,
    report: fn(Duration),
}

impl Turns {
    fn observe(&self, activity: CFRunLoopActivity, now: Instant) {
        if activity.intersects(STARTS) {
            self.started.set(Some(now));
            return;
        }
        if let Some(started) = self.started.take() {
            let ran = now.saturating_duration_since(started);
            if ran >= STALL {
                (self.report)(ran);
            }
        }
    }
}

/// Logs each stall of the main thread from now on. From the main thread, before the app runs.
pub fn watch_main() {
    if let Some(main) = CFRunLoop::main() {
        watch(&main, |ran| {
            log::warn!("the main thread was busy {} ms", ran.as_millis());
        });
    }
}

/// Times each turn of `run_loop` in its common modes, telling `report` of each stall. The
/// turn starts before any other observer of the loop and ends after every one, Core
/// Animation's commit of the frame included. The run loop keeps the observers.
fn watch(run_loop: &CFRunLoop, report: fn(Duration)) -> Vec<CFRetained<CFRunLoopObserver>> {
    let turns = Rc::new(Turns {
        started: Cell::new(None),
        report,
    });
    let observers = [(STARTS, isize::MIN), (ENDS, isize::MAX)].map(|(activities, order)| {
        let turns = Rc::clone(&turns);
        let observed = RcBlock::new(move |_: *mut CFRunLoopObserver, activity| {
            turns.observe(activity, Instant::now());
        });
        // SAFETY: the default allocator, and a block that's copied by the call and runs only
        // on the run loop's own thread, where the `Turns` it holds lives.
        unsafe { CFRunLoopObserver::with_handler(None, activities.0, true, order, Some(&observed)) }
    });
    // SAFETY: a mode CoreFoundation makes once and keeps for the life of the process.
    let common = unsafe { kCFRunLoopCommonModes };
    let observers: Vec<_> = observers.into_iter().flatten().collect();
    for observer in &observers {
        run_loop.add_observer(Some(observer), common);
    }
    observers
}

#[cfg(test)]
mod tests;
