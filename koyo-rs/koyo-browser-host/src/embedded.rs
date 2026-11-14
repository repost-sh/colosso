//! The host inside Koyo's own process, where pages are native (`docs/native-pages.md`).
//!
//! An `NSView` can't be shown in another process's window, so for native pages the browser
//! process is the app. The app hands [`start`] its end of the socket it would have given a
//! host process, and the host runs on the app's main thread from then on: CEF is loaded from
//! the engine the app carries and started there, its work run from `AppKit`'s run loop
//! (`pump`), and `NSApp` given what Chromium asks of it (`nsapp`). The helpers are still the
//! engine's, running this crate's binary. Everything else is the host as it is in a process of
//! its own, the app talking to it over the same protocol.
//!
//! CEF starts once in a process: a second [`start`] is refused, and when the host stops
//! ([`finish`]) the app is quitting.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use koyo_browser_ipc::{self as ipc, HostEvent};

mod nsapp;
pub(crate) mod pump;

pub(crate) use nsapp::{install as adopt_app, showing};

/// What the app gives the host to start it in the app's process.
pub struct Start {
    /// The host's end of the app's socket.
    pub stream: UnixStream,
    /// The token the app says hello with.
    pub token: String,
    /// CEF's root cache: one folder per profile inside it.
    pub profiles: PathBuf,
    /// The engine's `Koyo Browser Host.app`, which carries CEF and its helpers. The app has
    /// to run from a bundle that holds it, or the helpers don't start.
    pub engine: PathBuf,
}

/// The engine the host runs from, once it was started in the app's process: set once, as CEF
/// starts once in a process.
static ENGINE: OnceLock<PathBuf> = OnceLock::new();

/// The engine's bundle, when the host runs in the app's process.
pub(crate) fn engine() -> Option<&'static Path> {
    ENGINE.get().map(PathBuf::as_path)
}

/// Starts the host in this process, from any thread: it starts on the main thread, from the
/// run loop, and says so on the socket as a host process would. A start that fails says why
/// there too.
pub fn start(start: Start) {
    // Read and written here, off the main thread, before CEF opens the profiles.
    crate::host::drop_cleared(&start.profiles);
    forget_sessions(&start.profiles);
    on_main(move || {
        if ENGINE.set(start.engine.clone()).is_err() {
            let mut stream = start.stream;
            let _ = ipc::write_frame(
                &mut stream,
                &HostEvent::Error {
                    pane: None,
                    message: "The browser engine runs once while Koyo does: restart Koyo.".into(),
                },
            );
            return;
        }
        if let Err(why) = crate::host::run_embedded(start) {
            log::error!("the browser engine didn't start in the app: {why}");
        }
    });
}

pub use pump::on_main;

/// Stops the host as the app quits, on the main thread: runs CEF's work until every page has
/// closed, for at most `grace`, then shuts CEF down. The app has sent the host its shutdown
/// request already. Pages still open after `grace` are left as they are, CEF with them, and
/// the process's end takes both.
pub fn finish(grace: Duration) {
    if !pump::is_started() {
        return;
    }
    let began = Instant::now();
    let deadline = began + grace;
    // Every page's browser gone, and every window CEF made for one: it must outlive neither.
    let closed = || crate::host::all_closed() && crate::native::windows() == 0;
    // The main thread has nothing else to do as the app quits: CEF's work runs from here, a
    // millisecond's wait between turns (on a channel nothing sends on).
    let (_never, wait) = std::sync::mpsc::channel::<()>();
    while !closed() && Instant::now() < deadline {
        pump::work();
        let _ = wait.recv_timeout(Duration::from_millis(1));
    }
    if !closed() {
        say(&format!(
            "the browser engine's pages didn't close in time ({} windows left); leaving them to the exit",
            crate::native::windows()
        ));
        return;
    }
    // A few turns more, for what closing left to do.
    for _ in 0..5 {
        pump::work();
    }
    crate::host::teardown();
    pump::stop();
    cef::shutdown();
    say(&format!("the browser engine stopped in {} ms", began.elapsed().as_millis()));
}

/// Says `what` in the log and has it written now: the app is on its way out.
fn say(what: &str) {
    log::info!("{what}");
    log::logger().flush();
}

/// Has Chrome bring no window back: each profile's saved session goes, and the profile says
/// it left cleanly. A Chrome-style page is a Chrome browser, which Chrome saves with its
/// session; after a run that didn't quit cleanly it restores that session into windows of its
/// own the moment the first browser is made. Koyo brings its own tabs back (`docs/browser.md`).
fn forget_sessions(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for profile in entries.flatten().map(|entry| entry.path()) {
        let preferences = profile.join("Preferences");
        if !preferences.is_file() {
            continue;
        }
        let _ = std::fs::remove_dir_all(profile.join("Sessions"));
        let Ok(text) = std::fs::read_to_string(&preferences) else {
            continue;
        };
        let Ok(mut root) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(state) = root.get_mut("profile").and_then(|p| p.as_object_mut()) else {
            continue;
        };
        let clean = state.get("exit_type").and_then(|v| v.as_str()) == Some("Normal")
            && state.get("exited_cleanly").and_then(serde_json::Value::as_bool) != Some(false);
        if clean {
            continue;
        }
        state.insert("exit_type".into(), "Normal".into());
        state.insert("exited_cleanly".into(), true.into());
        if let Ok(text) = serde_json::to_string(&root) {
            let _ = std::fs::write(&preferences, text);
        }
    }
}

/// Loads CEF's framework from the engine, where it stays for the process's life.
pub(crate) fn load(engine: &Path) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt as _;
    let binary = framework(engine).join("Chromium Embedded Framework");
    let name =
        std::ffi::CString::new(binary.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // SAFETY: a path to the framework's binary, zero-terminated, alive through the call.
    if unsafe { cef::load_library(Some(&*name.as_ptr().cast())) } != 1 {
        return Err(format!("cannot load {}", binary.display()));
    }
    Ok(())
}

/// The engine's framework folder.
pub(crate) fn framework(engine: &Path) -> PathBuf {
    engine.join("Contents/Frameworks/Chromium Embedded Framework.framework")
}

/// The engine's helper, which Chromium starts its other helpers beside.
pub(crate) fn helper(engine: &Path) -> PathBuf {
    engine.join(
        "Contents/Frameworks/Koyo Browser Host Helper.app/Contents/MacOS/Koyo Browser Host Helper",
    )
}
