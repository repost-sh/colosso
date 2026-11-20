//! What the pump (`embedded/pump.rs`) owes the app's main thread, seen from the main thread's
//! run loop as `NSApp` runs it: a page on screen that nothing happens in costs it about one
//! wakeup a second, and work Chromium puts off still runs once it's due, though CEF never says
//! when that is.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use cef::ThreadId;
use koyo_browser_host::embedded;
use koyo_browser_ipc::AppRequest;
use koyo_browser_ipc::client::{Connection, Incoming};

use crate::inside::open;

/// How long a page just loaded is left to finish what its load set going.
const SETTLE: Duration = Duration::from_secs(1);

/// How long the main thread's wakeups are counted for at a time.
const WINDOW: Duration = Duration::from_secs(2);

/// How long Chromium may take to go quiet once the page has loaded: what the load set going
/// (its caches and history written, among them) takes longer on a busy Mac.
const QUIET_WITHIN: Duration = Duration::from_secs(40);

/// The most wakeups a second an idle page on screen may cost: the pump's one, and what else
/// wakes a run loop `NSApp` runs. It was over thirty, the pump running CEF's work every
/// thirtieth of a second whether or not there was any.
const MOST: f64 = 4.;

/// How long Chromium's work is put off for.
const DELAY: Duration = Duration::from_millis(200);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRunLoopGetMain() -> *mut c_void;
    fn CFRunLoopObserverCreate(
        allocator: *const c_void,
        activities: usize,
        repeats: u8,
        order: isize,
        callout: extern "C" fn(*mut c_void, usize, *mut c_void),
        context: *mut c_void,
    ) -> *mut c_void;
    fn CFRunLoopAddObserver(run_loop: *mut c_void, observer: *mut c_void, mode: *const c_void);
    fn CFRelease(object: *mut c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

/// `kCFRunLoopAfterWaiting`: the run loop woke from its wait.
const AFTER_WAITING: usize = 1 << 6;

/// How many times the main thread's run loop has woken since it was first watched.
static WAKES: AtomicU64 = AtomicU64::new(0);

extern "C" fn woke(_: *mut c_void, _: usize, _: *mut c_void) {
    WAKES.fetch_add(1, Ordering::Relaxed);
}

/// Counts each wakeup of the main thread's run loop from now on, in every mode `NSApp` runs it
/// in.
fn watch_wakes() -> Result<(), String> {
    let (done, watching) = mpsc::channel();
    embedded::on_main(move || {
        // SAFETY: CF's run-loop calls on the main thread's run loop, which keeps the observer
        // it's given, so the one reference made here goes.
        unsafe {
            let observer = CFRunLoopObserverCreate(
                std::ptr::null(),
                AFTER_WAITING,
                1,
                0,
                woke,
                std::ptr::null_mut(),
            );
            if !observer.is_null() {
                CFRunLoopAddObserver(CFRunLoopGetMain(), observer, kCFRunLoopCommonModes);
                CFRelease(observer);
            }
        }
        let _ = done.send(());
    });
    watching
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| format!("the main thread didn't start watching its wakeups: {error}"))
}

/// Waits `long` on the thread that drives the engine.
fn idle(long: Duration) {
    let (_never, tick) = mpsc::channel::<()>();
    let _ = tick.recv_timeout(long);
}

/// Opens the page, shows it, and leaves the engine to go quiet.
fn quiet_on_screen(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    host.send(&AppRequest::Visible {
        pane: 1,
        visible: true,
    })
    .map_err(|error| format!("visible: {error}"))?;
    idle(SETTLE);
    Ok(())
}

/// A page on screen that nothing happens in wakes the main thread about once a second, once
/// Chromium has finished what the page's load set going: measured in windows of [`WINDOW`]
/// until one is that quiet.
pub fn idle_on_screen(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    watch_wakes()?;
    quiet_on_screen(host, incoming)?;
    let deadline = Instant::now() + QUIET_WITHIN;
    let mut windows = Vec::new();
    while Instant::now() < deadline {
        let per_second = wakes_a_second(WINDOW);
        windows.push(format!("{per_second:.1}"));
        if per_second <= MOST {
            eprintln!(
                "an idle page on screen woke the main thread {per_second:.1} times a second \
                 (each {WINDOW:?} since it loaded: {})",
                windows.join(", ")
            );
            return Ok(());
        }
    }
    Err(format!(
        "an idle page on screen woke the main thread over {MOST} times a second in each {WINDOW:?} \
         for {QUIET_WITHIN:?}: {}",
        windows.join(", ")
    ))
}

/// How many times a second the main thread's run loop woke over the next `window`.
fn wakes_a_second(window: Duration) -> f64 {
    let before = WAKES.load(Ordering::Relaxed);
    idle(window);
    let woken = WAKES.load(Ordering::Relaxed) - before;
    #[expect(clippy::cast_precision_loss, reason = "a count of wakeups in seconds")]
    let woken = woken as f64;
    woken / window.as_secs_f64()
}

/// When the task put off was posted, from within CEF's work, and when it ran.
static PUT_OFF: Mutex<(Option<Instant>, Option<Instant>)> = Mutex::new((None, None));

/// The work put off, `PutsOff` posting `Due` for [`DELAY`] from within CEF's work: each says
/// when it ran.
#[expect(
    clippy::transmute_ptr_to_ptr,
    reason = "cef's wrap_* macros transmute references in what they expand to"
)]
mod tasks {
    use super::{DELAY, Instant, PUT_OFF, ThreadId};
    use cef::{ImplTask, Task, WrapTask, rc::Rc, wrap_task};

    wrap_task! {
        pub struct Due;

        impl Task {
            fn execute(&self) {
                if let Ok(mut put_off) = PUT_OFF.lock() {
                    put_off.1 = Some(Instant::now());
                }
            }
        }
    }

    wrap_task! {
        pub struct PutsOff;

        impl Task {
            fn execute(&self) {
                if let Ok(mut put_off) = PUT_OFF.lock() {
                    put_off.0 = Some(Instant::now());
                }
                let mut due = Due::new();
                let delay = i64::try_from(DELAY.as_millis()).unwrap_or(i64::MAX);
                cef::post_delayed_task(ThreadId::UI, Some(&mut due), delay);
            }
        }
    }
}

/// Work Chromium puts off from within its own, as its timers do, runs once it's due and late by
/// no more than it was put off for, on an engine that has gone quiet with a page on screen.
/// CEF says when it next has work only for work put off outside its own, so the pump runs it
/// unasked.
pub fn put_off(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    quiet_on_screen(host, incoming)?;
    let asked = Instant::now();
    let mut task = tasks::PutsOff::new();
    cef::post_task(ThreadId::UI, Some(&mut task));
    let deadline = asked + 3 * DELAY + Duration::from_secs(2);
    let (posted, ran) = loop {
        let times = PUT_OFF.lock().map(|times| *times).unwrap_or_default();
        if let (Some(posted), Some(ran)) = times {
            break (posted, ran);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "work put off for {DELAY:?} hadn't run {:?} after it was asked for",
                asked.elapsed()
            ));
        }
        idle(Duration::from_millis(5));
    };
    let after = ran - posted;
    eprintln!("work put off for {DELAY:?} ran {after:?} after it was put off");
    if after < DELAY || after > 2 * DELAY {
        return Err(format!(
            "work put off for {DELAY:?} ran {after:?} after it was put off, not between {DELAY:?} and {:?}",
            2 * DELAY
        ));
    }
    Ok(())
}
