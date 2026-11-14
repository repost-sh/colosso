//! The host's start and end: hello on the app's socket, CEF started in a process of the
//! host's own or in the app's (`embedded.rs`), and what stops it.
use super::{WRITER, chromium_version, engine_version, read_requests, send, with};
use cef::args::Args;
use cef::{CefString, Settings};
use koyo_browser_ipc::{self as ipc, HostEvent};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .map_err(|error| format!("{name} is not set ({error}); the host is started by Koyo"))
}

/// Says hello on the app's socket: reads the app's, checks it, and keeps the socket for
/// events. The reader's end comes back.
fn connect(mut stream: UnixStream, token: &str) -> Result<UnixStream, String> {
    let hello: ipc::Hello = ipc::read_frame(&mut stream)
        .map_err(|e| format!("hello: {e}"))?
        .ok_or("the app closed before hello")?;
    if let Err(message) = ipc::check_hello(&hello, token) {
        let _ = ipc::write_frame(
            &mut stream,
            &HostEvent::Error {
                pane: None,
                message: message.clone(),
            },
        );
        return Err(message);
    }
    crate::passkeys::enable(hello.passkeys);
    let reader = stream.try_clone().map_err(|e| e.to_string())?;
    let _ = WRITER.set(Mutex::new(stream));
    Ok(reader)
}

/// Finds where off-screen pages' surfaces go, and the GPU they're made on.
#[cfg(target_os = "macos")]
fn surfaces(service: &str) -> Result<(), String> {
    let sender = ipc::mach::Sender::look_up(service).map_err(|e| e.to_string())?;
    let gpu = crate::surfaces::Gpu::new().ok_or("no Metal device")?;
    with(|host| {
        host.surfaces = Some(sender);
        host.gpu = Some(gpu);
    });
    Ok(())
}

/// Starts CEF on `profiles`: as the browser process of its own, or in the app's
/// (`embedded.rs`), where the app runs the loop and CEF is found in the engine.
fn initialize(args: &Args, profiles: &std::path::Path) -> Result<(), String> {
    let mut app = crate::app::HostApp::new();
    std::fs::create_dir_all(profiles).map_err(|e| format!("{}: {e}", profiles.display()))?;
    // CEF ignores relative cache paths and falls back to memory.
    let profiles = profiles
        .canonicalize()
        .map_err(|e| format!("{}: {e}", profiles.display()))?;
    with(|host| host.profiles.clone_from(&profiles));
    let path = |p: PathBuf| CefString::from(p.to_string_lossy().as_ref());
    #[cfg(target_os = "macos")]
    let engine = crate::embedded::engine();
    let settings = Settings {
        // No page is rendered off screen in the app's process, and CEF says not to turn
        // this on then.
        windowless_rendering_enabled: i32::from(!super::native()),
        root_cache_path: path(profiles.clone()),
        cache_path: path(profiles.join("default")),
        persist_session_cookies: 1,
        log_file: path(profiles.join("host.log")),
        log_severity: cef::LogSeverity::WARNING,
        // Opaque white until a page paints, as in a browser tab.
        background_color: 0xFFFF_FFFF,
        keychain_service_name: "Koyo Browser Engine".into(),
        // CEF takes the outermost app bundle for its own, which inside Koyo.app is Koyo's,
        // and looks for its framework and helpers there.
        #[cfg(target_os = "macos")]
        main_bundle_path: crate::own_bundle().unwrap_or_default().as_str().into(),
        // In the app's process the app runs the loop, and CEF isn't beside the executable.
        #[cfg(target_os = "macos")]
        external_message_pump: i32::from(engine.is_some()),
        #[cfg(target_os = "macos")]
        framework_dir_path: engine
            .map(|engine| path(crate::embedded::framework(engine)))
            .unwrap_or_default(),
        #[cfg(target_os = "macos")]
        browser_subprocess_path: engine
            .map(|engine| path(crate::embedded::helper(engine)))
            .unwrap_or_default(),
        ..Settings::default()
    };
    if cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("CEF did not initialize".into());
    }
    Ok(())
}

/// Tells the app the host is up, and starts taking its requests.
fn serve(reader: UnixStream) -> Result<(), String> {
    send(&HostEvent::Ready {
        protocol: ipc::PROTOCOL,
        engine: engine_version(),
        chromium: chromium_version(),
    });
    std::thread::Builder::new()
        .name("app-socket".into())
        .spawn(move || read_requests(reader))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Lets go of what holds CEF's objects: it waits in shutdown for every reference to go.
pub(crate) fn teardown() {
    crate::dialogs::clear();
    crate::menus::clear();
    #[cfg(target_os = "macos")]
    crate::native::clear();
    with(|host| {
        host.panes.clear();
        host.contexts.clear();
    });
}

/// The browser process's whole life.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let socket = env(ipc::ENV_SOCKET_FD)?;
    let token = env(ipc::ENV_TOKEN)?;
    let profiles = PathBuf::from(env(ipc::ENV_PROFILE_ROOT)?);
    let stream =
        ipc::inherit::take(&socket).map_err(|e| format!("the app's socket ({socket}): {e}"))?;
    let reader = connect(stream, &token)?;
    #[cfg(target_os = "macos")]
    {
        crate::mac_app::install()?;
        surfaces(&env(ipc::ENV_SURFACE_SERVICE)?)?;
    }
    let mut app = crate::app::HostApp::new();
    let code = cef::execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        return Err(format!("started as a subprocess ({code})"));
    }
    initialize(args, &profiles)?;
    serve(reader)?;
    cef::run_message_loop();
    teardown();
    cef::shutdown();
    Ok(())
}

/// The host's start in the app's own process, on its main thread (`embedded.rs`): the same
/// hello, then CEF loaded from the engine and started with the app running the loop. What
/// goes wrong once the app is heard is said on the socket, as a host process would say it.
#[cfg(target_os = "macos")]
pub(crate) fn run_embedded(start: crate::embedded::Start) -> Result<(), String> {
    let crate::embedded::Start {
        stream,
        token,
        profiles,
        engine,
    } = start;
    let reader = connect(stream, &token)?;
    let started = (|| {
        crate::embedded::load(&engine)?;
        // The API version this build was compiled against, before any other call.
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        crate::embedded::adopt_app()?;
        // CEF reads the process's arguments as Chromium's switches; the app's are none.
        let args: &'static Args = Box::leak(Box::new(Args::new()));
        initialize(args, &profiles)?;
        crate::embedded::pump::started();
        serve(reader)
    })();
    if let Err(message) = &started {
        send(&HostEvent::Error {
            pane: None,
            message: message.clone(),
        });
    }
    started
}

/// Every pane has closed after a shutdown was asked for: the loop ends. In the app's process
/// the app runs the loop, and stops CEF once it sees the same (`embedded::finish`).
pub(crate) fn quit() {
    if !super::native() {
        cef::quit_message_loop();
    }
}
