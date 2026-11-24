//! What the pump (`embedded/pump.rs`) owes the app's main thread, seen from the main thread's
//! run loop as `NSApp` runs it: a page on screen that nothing happens in leaves it asleep, and
//! work Chromium puts off still runs once it's due. That holds where the engine's framework says
//! each time when Chromium next has work (`script/cef-patches/external-pump-wake-ups.patch`);
//! on CEF's own build, which says it only for some work, the pump's net wakes the main thread
//! about once a second and runs the rest.

use std::ffi::c_void;
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
const WINDOW: Duration = Duration::from_secs(10);

/// How long Chromium may take to go quiet once the page has loaded: what the load set going
/// (its caches and history written, among them) and what a profile made for the test sets up
/// (its databases, thousands of tasks on the main thread) take longer on a busy Mac.
const QUIET_WITHIN: Duration = Duration::from_secs(60);

/// The most wakeups a second an idle page on screen may cost where CEF says when Chromium next
/// has work: Chromium's own timers, a second or two apart, the short chains they set off, and
/// whatever else wakes a run loop `NSApp` runs. Measured 1.0 to 1.9 a second, with bursts of 4
/// or 5, once the profile had set itself up (2026-11-24, `docs/native-pages.md` 3.1).
const MOST: f64 = 3.;

/// The most on CEF's own build: the net's one, and the rest. It was over thirty, the pump
/// running CEF's work every thirtieth of a second whether or not there was any.
const MOST_ON_A_NET: f64 = 4.;

/// The engine's framework.
const FRAMEWORK: &str = "Contents/Frameworks/Chromium Embedded Framework.framework";

/// The framework's manifest (`script/cef-framework`), which lists its patches.
const MANIFEST: &str = "Resources/koyo-build.txt";

/// How long Chromium's work is put off for.
const DELAY: Duration = Duration::from_millis(200);

/// How long a task puts off the next, as a chain of Chromium's timers does: less than CEF's time
/// slice (`MessagePumpExternal`'s 10 ms), so the next falls due within the turn that found
/// Chromium idle, which still has to ask for the wakeups of what the next queues.
const LATE_IN_TURN: Duration = Duration::from_millis(2);

/// How many times work is put off from late in a turn.
const ROUNDS: usize = 8;

/// How long the engine is left between rounds: a few of CEF's time slices, so each round's
/// first task starts a turn of its own.
const BETWEEN_ROUNDS: Duration = Duration::from_millis(30);

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
pub fn idle(long: Duration) {
    let (_never, tick) = mpsc::channel::<()>();
    let _ = tick.recv_timeout(long);
}

/// Opens the page, shows it, and leaves the engine to go quiet.
fn quiet_on_screen(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    host.send(&AppRequest::Sight { pane: 1, kept: true, seen: true })
    .map_err(|error| format!("visible: {error}"))?;
    idle(SETTLE);
    Ok(())
}

/// Whether the engine's framework says each time when Chromium next has work: whether its
/// manifest lists `external-pump-wake-ups`.
fn says_when() -> Result<bool, String> {
    let manifest = crate::inside::engine()?.join(FRAMEWORK).join(MANIFEST);
    match std::fs::read_to_string(&manifest) {
        Ok(text) => Ok(text.lines().any(|line| line.starts_with("patch external-pump-wake-ups "))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{}: {error}", manifest.display())),
    }
}

/// A page on screen that nothing happens in leaves the main thread asleep but for Chromium's own
/// work, or wakes it about once a second on CEF's own build, once Chromium has finished what the
/// page's load set going: measured in windows of [`WINDOW`] until one is that quiet.
pub fn idle_on_screen(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    let most = if says_when()? { MOST } else { MOST_ON_A_NET };
    watch_wakes()?;
    quiet_on_screen(host, incoming)?;
    let deadline = Instant::now() + QUIET_WITHIN;
    let mut windows = Vec::new();
    while Instant::now() < deadline {
        let per_second = wakes_a_second(WINDOW);
        windows.push(format!("{per_second:.1}"));
        if per_second <= most {
            eprintln!(
                "an idle page on screen woke the main thread {per_second:.1} times a second \
                 (each {WINDOW:?} since it loaded: {})",
                windows.join(", ")
            );
            return Ok(());
        }
    }
    Err(format!(
        "an idle page on screen woke the main thread over {most} times a second in each {WINDOW:?} \
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

/// The work put off, `PutsOff` posting `Due` for [`DELAY`] from within CEF's work, each telling
/// `times` when it ran, and `PutsOffLateInTheTurn` posting `PutsOff` for [`LATE_IN_TURN`].
#[expect(
    clippy::transmute_ptr_to_ptr,
    reason = "cef's wrap_* macros transmute references in what they expand to"
)]
mod tasks {
    use std::sync::mpsc::Sender;

    use super::{DELAY, Instant, LATE_IN_TURN, ThreadId};
    use cef::{ImplTask, Task, WrapTask, rc::Rc, wrap_task};

    /// `duration` in milliseconds, as CEF takes a delay.
    fn millis(duration: std::time::Duration) -> i64 {
        i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
    }

    wrap_task! {
        pub struct Due {
            times: Sender<Instant>,
        }

        impl Task {
            fn execute(&self) {
                let _ = self.times.send(Instant::now());
            }
        }
    }

    wrap_task! {
        pub struct PutsOff {
            times: Sender<Instant>,
        }

        impl Task {
            fn execute(&self) {
                let _ = self.times.send(Instant::now());
                let mut due = Due::new(self.times.clone());
                cef::post_delayed_task(ThreadId::UI, Some(&mut due), millis(DELAY));
            }
        }
    }

    wrap_task! {
        pub struct PutsOffLateInTheTurn {
            times: Sender<Instant>,
        }

        impl Task {
            fn execute(&self) {
                let mut task = PutsOff::new(self.times.clone());
                cef::post_delayed_task(ThreadId::UI, Some(&mut task), millis(LATE_IN_TURN));
            }
        }
    }
}

/// How long after it was put off the work `PutsOff` put off ran, as `times` heard, or `None` if
/// it hadn't run well after it was due.
fn put_off_ran_after(times: &Receiver<Instant>) -> Option<Duration> {
    let deadline = Instant::now() + 3 * DELAY + Duration::from_secs(2);
    let heard = || times.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok();
    let posted = heard()?;
    let ran = heard()?;
    Some(ran - posted)
}

/// Whether work put off for [`DELAY`] ran on time: once it was due, and late by no more than it
/// was put off for.
fn on_time(after: Duration) -> bool {
    (DELAY..=2 * DELAY).contains(&after)
}

/// Work Chromium puts off from within its own, as its timers do, runs once it's due and late by
/// no more than it was put off for, on an engine that has gone quiet with a page on screen.
/// Koyo's framework asks for its turn; on CEF's own build, which doesn't, the pump's net runs it.
pub fn put_off(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    quiet_on_screen(host, incoming)?;
    let asked = Instant::now();
    let (times, heard) = mpsc::channel();
    let mut task = tasks::PutsOff::new(times);
    cef::post_task(ThreadId::UI, Some(&mut task));
    let Some(after) = put_off_ran_after(&heard) else {
        return Err(format!(
            "work put off for {DELAY:?} hadn't run {:?} after it was asked for",
            asked.elapsed()
        ));
    };
    eprintln!("work put off for {DELAY:?} ran {after:?} after it was put off");
    if !on_time(after) {
        return Err(format!(
            "work put off for {DELAY:?} ran {after:?} after it was put off, not between {DELAY:?} and {:?}",
            2 * DELAY
        ));
    }
    Ok(())
}

/// Work put off by a task that runs late in a turn, after Chromium found the turn idle and
/// before CEF's time slice ends, runs once it's due, as work put off in any other turn does. On
/// an engine gone quiet with a page on screen, nothing else wakes the main thread for it.
pub fn put_off_late_in_a_turn(
    host: &mut Connection,
    incoming: &Receiver<Incoming>,
) -> Result<(), String> {
    quiet_on_screen(host, incoming)?;
    let mut rounds = Vec::new();
    for _ in 0..ROUNDS {
        let (times, heard) = mpsc::channel();
        let mut task = tasks::PutsOffLateInTheTurn::new(times);
        cef::post_task(ThreadId::UI, Some(&mut task));
        rounds.push(put_off_ran_after(&heard));
        idle(BETWEEN_ROUNDS);
    }
    let said = (rounds.iter())
        .map(|after| after.map_or_else(|| "never".to_owned(), |after| format!("{after:?}")))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("work put off for {DELAY:?} late in a turn ran after: {said}");
    let late = rounds.iter().filter(|after| !after.is_some_and(on_time)).count();
    if late > 0 {
        return Err(format!(
            "work put off for {DELAY:?} late in a turn ran off time, or never, in {late} of \
             {ROUNDS} rounds: {said}"
        ));
    }
    Ok(())
}
