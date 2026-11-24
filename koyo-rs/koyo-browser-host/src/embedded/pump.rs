//! CEF's work, run from `AppKit`'s run loop on the main thread, which the app runs: CEF says when
//! it next has work (`OnScheduleMessagePumpWork`, from any thread) and a run-loop timer in the
//! common modes runs it then. The timer fires during a menu's or a window's tracking too, as
//! those run the loop in modes the common ones include.
//!
//! Koyo's framework says it for all of CEF's work (`external-pump-wake-ups`, one of
//! `script/cef-patches`): a turn of CEF's work ends by asking for the wakeup Chromium's last task
//! left it, as Chromium's own pumps do. So there the timer fires only when CEF asks, and an
//! engine nothing happens in leaves the main thread asleep.
//!
//! CEF's own build, which the host still runs with a warning, doesn't, so there the timer fires
//! at the latest a while on too, the net. A turn of it (`MessagePumpExternal` in CEF's
//! `browser_message_loop.cc`) runs work for 10 ms, and work still waiting then asks for no turn,
//! nor does any posted after it, until a turn runs. And work put off from within a turn, as
//! nearly every timer of Chromium's on this thread is, never asks at all: Chromium leaves such a
//! wakeup to the pump, which CEF's doesn't keep. With no net, work put off for 200 ms hadn't run
//! three seconds on (2026-11-19); `tests/embedded/pump.rs` has it run on time. So after a turn
//! someone asked for, or one that ran out of time, the net is soon ([`Load::floor`]): a
//! thirtieth of a second, as CEF advises, while a person sees what the engine does or waits on
//! it. Each turn nobody asked for doubles it, up to a second ([`Load::ceiling`]), so an engine
//! nothing happens in costs the main thread a wakeup a second rather than thirty. Asked for, it's
//! soon again. What that costs in lateness: work put off from a turn someone asked for, or one
//! that ran out of time, runs late by no more than it was put off for and a thirtieth of a
//! second, or a second; work put off from a turn nobody asked for, as each link of a chain of
//! timers is after the first, runs late by up to the net after that turn, which has grown, so by
//! up to a second however little it was put off for.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Never run CEF's work further apart than this just after a turn a person may see the end of.
const AT_MOST: f64 = 1. / 30.;

/// Never run it further apart than this.
const CEILING: f64 = 1.;

/// Koyo's patch with which CEF says when it next has work after every turn of it.
const WAKE_UPS: &str = "external-pump-wake-ups";

/// How long a turn of CEF's work runs work for at most (`MessagePumpExternal`'s time slice): one
/// this long may have left some.
const SLICE: Duration = Duration::from_millis(10);

/// What the engine works for, which sets how far apart its work may run (`host::load`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load {
    /// No page, no window, nothing of the app's waiting.
    Idle,
    /// Pages, none of them on screen, and nothing else.
    Unseen,
    /// A page on screen or being opened, and nothing else.
    Shown,
    /// The engine starting, a request of the app's waiting for it, or a window of Chrome's own:
    /// Chrome's own windows draw from timers on this thread.
    Waited,
}

impl Load {
    /// How soon CEF's work runs again at the latest after a turn someone asked for, or one that
    /// ran out of time, in seconds.
    pub(crate) fn floor(self) -> f64 {
        match self {
            Self::Idle => CEILING,
            Self::Unseen => 1. / 10.,
            Self::Shown | Self::Waited => AT_MOST,
        }
    }

    /// How far apart CEF's work may run while nobody asks for any, in seconds.
    pub(crate) fn ceiling(self) -> f64 {
        match self {
            Self::Waited => AT_MOST,
            Self::Idle | Self::Unseen | Self::Shown => CEILING,
        }
    }

    /// How soon CEF's work runs again at the latest after a turn that came `gap` after the one
    /// before: the floor after a turn someone asked for or that ran out of time, else twice
    /// `gap`, within the floor and the ceiling.
    pub(crate) fn net(self, gap: f64, asked: bool, ran_out: bool) -> f64 {
        if asked || ran_out {
            self.floor()
        } else {
            (gap * 2.).max(self.floor()).min(self.ceiling())
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
/// Whether CEF or the app asked for a turn since the timer last fired.
static WANTED: AtomicBool = AtomicBool::new(false);
/// Whether CEF's work is running, so a run loop it spins meanwhile doesn't run it again.
static WORKING: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// How long the net was after the last turn, in seconds: the main thread's.
    static GAP: Cell<f64> = const { Cell::new(AT_MOST) };
}

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

/// CEF has work in `delay_ms`, or the app has some for it; from any thread.
pub fn schedule(delay_ms: i64) {
    WANTED.store(true, Ordering::Release);
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
    let asked = WANTED.swap(false, Ordering::AcqRel);
    let later = (LATER.lock())
        .map(|mut later| std::mem::take(&mut *later))
        .unwrap_or_default();
    for then in later {
        then();
    }
    let began = Instant::now();
    work();
    let ran_out = began.elapsed() >= SLICE;
    if STARTED.load(Ordering::Acquire) && !crate::framework::patched(WAKE_UPS) {
        let net = crate::host::load().net(GAP.get(), asked, ran_out);
        GAP.set(net);
        // SAFETY: a CF call with no arguments.
        let now = unsafe { CFAbsoluteTimeGetCurrent() };
        arm(timer, now + net);
    }
}

#[cfg(test)]
mod tests;
