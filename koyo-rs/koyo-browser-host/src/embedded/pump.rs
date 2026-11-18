//! CEF's work, run from `AppKit`'s run loop on the main thread, which the app runs: CEF says when
//! it next has work (`OnScheduleMessagePumpWork`, from any thread) and a run-loop timer in the
//! common modes runs it then, and at the latest a while on ([`Load`]). The timer fires during a
//! menu's or a window's tracking too, as those run the loop in modes the common ones include.
//!
//! The latest is CEF's net for the work it leaves without asking: a turn runs its work for 10 ms
//! (`MessagePumpExternal` in CEF's `browser_message_loop.cc`), and the work still waiting then
//! asks for no turn, nor does any posted after it, until a turn runs. CEF advises a thirtieth of
//! a second, which holds while a person sees what the engine does or waits on it. A page out of
//! sight, loading say, can have such work too, which a tenth of a second on still finishes. With
//! no page at all, a second: what the engine schedules still runs when it asks, and the app's
//! requests, a page to open or to show among them, run it at once (`host::queue`).

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

/// Never run CEF's work further apart than this while a person sees what it does.
const AT_MOST: f64 = 1. / 30.;

/// What the engine works for, which sets how far apart its work may run (`host::load`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load {
    /// No page, no window, nothing of the app's waiting.
    Idle,
    /// Pages, none of them on screen, and nothing else.
    Unseen,
    /// A page on screen or being opened, or anything else a person waits on.
    Seen,
}

impl Load {
    /// How far apart CEF's work may run, in seconds.
    pub(crate) fn at_most(self) -> f64 {
        match self {
            Self::Idle => 1.,
            Self::Unseen => 1. / 10.,
            Self::Seen => AT_MOST,
        }
    }
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        fire_date: f64,
        interval: f64,
        flags: usize,
        order: isize,
        callout: extern "C" fn(*mut c_void, *mut c_void),
        context: *mut c_void,
    ) -> *mut c_void;
    fn CFRunLoopAddTimer(run_loop: *mut c_void, timer: *mut c_void, mode: *const c_void);
    fn CFRunLoopTimerSetNextFireDate(timer: *mut c_void, fire_date: f64);
    fn CFRunLoopWakeUp(run_loop: *mut c_void);
    fn CFRunLoopTimerInvalidate(timer: *mut c_void);
    fn CFRelease(object: *mut c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

static TIMER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// Whether CEF has started, and so has work to run.
static STARTED: AtomicBool = AtomicBool::new(false);

/// What to run on the main thread at the timer's next firing, outside whatever asked.
static LATER: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send>>> = std::sync::Mutex::new(Vec::new());

/// Runs `then` on the main thread from the run loop, soon: outside the app's own update and
/// frame, which whatever CEF does in answer (a window made, a run loop spun) mustn't be in.
/// From any thread.
pub fn on_main(then: impl FnOnce() + Send + 'static) {
    install();
    if let Ok(mut later) = LATER.lock() {
        later.push(Box::new(then));
    }
    schedule(0);
}

/// What a run-loop timer is made with (`CFRunLoopTimerContext`).
#[repr(C)]
struct TimerContext {
    version: isize,
    info: *mut c_void,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
}

/// Runs `tick` on the main thread every `interval` seconds, in the common modes, until it
/// answers false. From the main thread.
pub(crate) fn every(interval: f64, tick: fn() -> bool) {
    extern "C" fn fire(timer: *mut c_void, info: *mut c_void) {
        // SAFETY: `info` is the `fn() -> bool` the timer was made with, below.
        let tick: fn() -> bool = unsafe { std::mem::transmute(info) };
        if !tick() {
            // SAFETY: the timer firing, which the run loop holds until it's invalidated.
            unsafe { CFRunLoopTimerInvalidate(timer) };
        }
    }
    let mut context = TimerContext {
        version: 0,
        info: tick as *mut c_void,
        retain: std::ptr::null(),
        release: std::ptr::null(),
        copy_description: std::ptr::null(),
    };
    // SAFETY: CF's run-loop calls on the main thread's run loop; the context is copied by
    // the create call, and the run loop keeps the timer, so the one reference made here goes.
    unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + interval,
            interval,
            0,
            0,
            fire,
            (&raw mut context).cast(),
        );
        if !timer.is_null() {
            CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
            CFRelease(timer);
        }
    }
}

/// CEF has started: its work runs from here on.
pub fn started() {
    STARTED.store(true, Ordering::Release);
    schedule(0);
}

/// Whether CEF's work runs.
pub fn is_started() -> bool {
    STARTED.load(Ordering::Acquire)
}

/// CEF is about to shut down: no more of its work runs.
pub fn stop() {
    STARTED.store(false, Ordering::Release);
}

/// Runs CEF's work now, on the main thread, unless it's running already (a run loop spun
/// from inside it mustn't run it again) or CEF isn't running.
pub fn work() {
    if !STARTED.load(Ordering::Acquire) || WORKING.swap(true, Ordering::AcqRel) {
        return;
    }
    cef::do_message_loop_work();
    WORKING.store(false, Ordering::Release);
}

/// When the timer fires next, as an `f64`'s bits: the soonest of what was asked since it last
/// fired and its net; infinite while it fires.
static ASKED: AtomicU64 = AtomicU64::new(f64::INFINITY.to_bits());
/// Whether CEF's work is running, so a run loop it spins meanwhile doesn't run it again.
static WORKING: AtomicBool = AtomicBool::new(false);

/// Makes the timer, the first time, on the main thread's run loop; from any thread.
pub fn install() {
    if !TIMER.load(Ordering::Acquire).is_null() {
        return;
    }
    // SAFETY: CF's run-loop calls on the main thread's run loop, which lives as long as the
    // process; the timer is kept for as long too.
    unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + AT_MOST,
            1.0e9,
            0,
            0,
            fire,
            std::ptr::null_mut(),
        );
        if timer.is_null() {
            log::error!("browser engine: no run-loop timer for CEF's work");
            return;
        }
        CFRunLoopAddTimer(CFRunLoopGetMain(), timer, kCFRunLoopCommonModes);
        TIMER.store(timer, Ordering::Release);
    }
}

/// CEF has work in `delay_ms`; from any thread.
pub fn schedule(delay_ms: i64) {
    let timer = TIMER.load(Ordering::Acquire);
    if timer.is_null() {
        return;
    }
    // SAFETY: a CF call with no arguments.
    let when = unsafe { CFAbsoluteTimeGetCurrent() } + (delay_ms.max(0) as f64) / 1000.;
    arm(timer, when);
}

/// Has the timer fire at `when`, unless it fires sooner already; from any thread. A sooner time
/// asked meanwhile on another thread is armed again after this, so it holds whichever thread's
/// arming lands last.
fn arm(timer: *mut c_void, when: f64) {
    let sooner = |asked: u64| (when < f64::from_bits(asked)).then_some(when.to_bits());
    if ASKED.fetch_update(Ordering::AcqRel, Ordering::Acquire, sooner).is_err() {
        return;
    }
    let mut armed = when;
    loop {
        // SAFETY: CF's timers and run loops may be changed from any thread.
        unsafe { CFRunLoopTimerSetNextFireDate(timer, armed) };
        let asked = f64::from_bits(ASKED.load(Ordering::Acquire));
        if asked >= armed {
            break;
        }
        armed = asked;
    }
    // SAFETY: as above.
    unsafe { CFRunLoopWakeUp(CFRunLoopGetMain()) };
}

extern "C" fn fire(timer: *mut c_void, _: *mut c_void) {
    // SAFETY: a CF call with no arguments.
    let now = unsafe { CFAbsoluteTimeGetCurrent() };
    if WORKING.load(Ordering::Acquire) {
        // SAFETY: the timer is ours and alive.
        unsafe { CFRunLoopTimerSetNextFireDate(timer, now + AT_MOST / 2.) };
        return;
    }
    ASKED.store(f64::INFINITY.to_bits(), Ordering::Release);
    let later = (LATER.lock())
        .map(|mut later| std::mem::take(&mut *later))
        .unwrap_or_default();
    for then in later {
        then();
    }
    work();
    if STARTED.load(Ordering::Acquire) {
        // SAFETY: a CF call with no arguments.
        let now = unsafe { CFAbsoluteTimeGetCurrent() };
        arm(timer, now + crate::host::load().at_most());
    }
}

#[cfg(test)]
mod tests;
