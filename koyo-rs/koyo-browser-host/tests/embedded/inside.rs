//! A test, run inside its app: `NSApp` runs the main thread's loop while another thread
//! starts the engine in this process, as Koyo starts it (`koyo/src/browser/native`), opens a
//! page, has it do what the test is of, and stops the engine as Koyo does as it quits.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use koyo_browser_host::embedded;
use koyo_browser_ipc::client::{Connection, Embedded, Incoming, Launch};
use koyo_browser_ipc::{AppRequest, HostEvent, Viewport};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::NSPoint;

define_class!(
    /// The test app's `NSApp`, of a class of its own as Koyo's is of GPUI's, which the engine
    /// gives what Chromium asks of it (`embedded/nsapp.rs`). It never comes forward.
    #[unsafe(super(NSApplication))]
    #[name = "KoyoEngineTestApplication"]
    struct TestApplication;

    impl TestApplication {
        #[unsafe(method(activateIgnoringOtherApps:))]
        fn activate_ignoring_other_apps(&self, _ignoring: Bool) {}

        #[unsafe(method(activate))]
        fn activate(&self) {}
    }

    unsafe impl NSObjectProtocol for TestApplication {}
);

/// A page that says when it has loaded.
const PAGE: &str = "data:text/html,<title>embedded</title><body>hi</body>";

/// How many animation frames the page gets in half a second.
const FRAMES: &str = "new Promise(done => { let frames = 0; \
    const count = () => { frames += 1; requestAnimationFrame(count); }; \
    requestAnimationFrame(count); setTimeout(() => done(frames), 500); })";

/// How long the engine may take to do what the test waits for.
const PATIENCE: Duration = Duration::from_secs(30);

/// What a test has the page do once it's open, from the thread that drives the engine.
type Step = fn(&mut Connection, &Receiver<Incoming>) -> Result<(), String>;

/// The tests, by name, each with what it has the page do.
pub const TESTS: [(&str, Step); 4] = [
    (
        "a_page_loads_and_draws_in_the_engine_started_in_this_process",
        load_and_draw,
    ),
    (
        "an_idle_page_on_screen_wakes_the_main_thread_about_once_a_second",
        crate::pump::idle_on_screen,
    ),
    (
        "work_chromium_puts_off_runs_once_it_is_due",
        crate::pump::put_off,
    ),
    (
        "a_bubble_goes_with_its_page_out_of_the_app_window_and_back",
        crate::bubbles::leave_and_return,
    ),
];

/// Runs the test `test` with its profiles in `scratch`, on the main thread: how it ended.
pub fn run(scratch: &Path, test: &str) -> ExitCode {
    if MainThreadMarker::new().is_none() {
        eprintln!("the engine test runs on the main thread");
        return ExitCode::FAILURE;
    }
    let Some(&(_, step)) = TESTS.iter().find(|(name, _)| *name == test) else {
        eprintln!("no engine test {test}");
        return ExitCode::FAILURE;
    };
    // SAFETY: `sharedApplication` makes `NSApp` of the class it's sent to, on the main thread.
    let app: Retained<NSApplication> =
        unsafe { msg_send![TestApplication::class(), sharedApplication] };
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let (told, outcome) = mpsc::channel();
    let scratch = scratch.to_owned();
    let driving = std::thread::Builder::new()
        .name("engine-test".into())
        .spawn(move || {
            let _ = told.send(drive(&scratch, step));
            quit();
        });
    if let Err(error) = driving {
        eprintln!("no thread to drive the engine from: {error}");
        return ExitCode::FAILURE;
    }
    app.run();
    match outcome.try_recv() {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(why)) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("the app stopped before the test said how it ended: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The engine this app holds, as Koyo holds it.
fn engine() -> Result<PathBuf, String> {
    let this = std::env::current_exe().map_err(|error| error.to_string())?;
    let contents = this.ancestors().nth(2).ok_or("the test isn't in an app")?;
    Ok(contents.join("Frameworks/Koyo Browser Host.app"))
}

/// Starts the engine in this process, has its page do `step`, and stops it.
fn drive(scratch: &Path, step: Step) -> Result<(), String> {
    let engine = engine()?;
    let launch = Launch {
        executable: engine.join("Contents/MacOS/Koyo Browser Host"),
        profiles: scratch.join("profiles"),
        passkeys: false,
    };
    let (sink, incoming) = mpsc::channel();
    let sink = move |message| sink.send(message).err().map(|unsent| unsent.0);
    let profiles = launch.profiles.clone();
    let embedded = Embedded {
        start: Box::new(move |stream, token| {
            embedded::start(embedded::Start {
                stream,
                token,
                profiles,
                engine,
            });
        }),
        finish: Box::new(embedded::finish),
    };
    let began = Instant::now();
    let mut host = Connection::start_embedded(&launch, embedded, sink)
        .map_err(|error| format!("the engine didn't start in this process: {error}"))?;
    eprintln!(
        "the engine started in this process in {:?}",
        began.elapsed()
    );
    let shown = step(&mut host, &incoming);
    let stopped = stop(host);
    shown?;
    eprintln!("the engine stopped in {:?}", stopped?);
    Ok(())
}

/// Opens the page as pane 1 and waits for it to load.
pub fn open(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    let asked = Instant::now();
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 1.,
    };
    let open = AppRequest::Open {
        pane: 1,
        url: PAGE.into(),
        viewport,
        profile: "embedded".into(),
    };
    host.send(&open).map_err(|error| format!("open: {error}"))?;
    wait(incoming, "the page to load", |event| {
        let loaded = matches!(event, HostEvent::Page { pane: 1, state }
            if state.title == "embedded" && !state.loading);
        loaded.then_some(())
    })?;
    eprintln!(
        "the page loaded {:?} after it was asked for",
        asked.elapsed()
    );
    Ok(())
}

/// Opens the page, and has it count its frames.
fn load_and_draw(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    let count = AppRequest::Cdp {
        pane: 1,
        call: 1,
        caller: koyo_browser_ipc::CdpCaller::Agent,
        session: None,
        method: "Runtime.evaluate".into(),
        params: serde_json::json!({ "expression": FRAMES, "returnByValue": true, "awaitPromise": true }),
    };
    host.send(&count)
        .map_err(|error| format!("evaluate: {error}"))?;
    let (result, error) = wait(
        incoming,
        "the page to count its frames",
        |event| match event {
            HostEvent::CdpReply {
                call: 1,
                result,
                error,
                ..
            } => Some((result.clone(), error.clone())),
            _ => None,
        },
    )?;
    if let Some(error) = error {
        return Err(format!("the page couldn't count its frames: {error}"));
    }
    let drawn = (result.as_ref())
        .and_then(|result| result["result"]["value"].as_u64())
        .ok_or_else(|| format!("no count of frames in {result:?}"))?;
    if drawn <= 15 {
        return Err(format!("the page drew {drawn} frames in half a second"));
    }
    eprintln!("the page drew {drawn} frames in half a second");
    Ok(())
}

/// What the engine sends until `want` takes an event, for [`PATIENCE`] at most.
fn wait<T>(
    incoming: &Receiver<Incoming>,
    what: &str,
    mut want: impl FnMut(&HostEvent) -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match incoming.recv_timeout(left) {
            Ok(Incoming::Event(HostEvent::Error { pane, message })) => {
                return Err(format!(
                    "waiting for {what}, the engine failed on pane {pane:?}: {message}"
                ));
            }
            Ok(Incoming::Event(event)) => {
                if let Some(found) = want(&event) {
                    return Ok(found);
                }
            }
            Ok(Incoming::Closed) => return Err(format!("the engine went away waiting for {what}")),
            Err(error) => return Err(format!("waiting for {what}: {error}")),
        }
    }
}

/// Stops the engine on the main thread, as Koyo does as it quits: how long it took.
fn stop(host: Connection) -> Result<Duration, String> {
    let began = Instant::now();
    let (done, stopped) = mpsc::channel();
    embedded::on_main(move || {
        host.shutdown(Duration::from_secs(10));
        let _ = done.send(());
    });
    stopped
        .recv_timeout(PATIENCE)
        .map_err(|error| format!("the engine didn't stop: {error}"))?;
    Ok(began.elapsed())
}

/// Ends `NSApp`'s run, from the main thread's loop.
fn quit() {
    embedded::on_main(|| {
        let Some(main) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(main);
        app.stop(None);
        let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::ZERO,
            NSEventModifierFlags::empty(),
            0.,
            0,
            None,
            0,
            0,
            0,
        );
        if let Some(event) = event {
            app.postEvent_atStart(&event, true);
        }
    });
}
