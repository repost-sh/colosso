//! The host's start and end: hello on the app's socket, CEF started in a process of the
//! host's own or in the app's (`embedded.rs`), and what stops it.
use super::{WRITER, chromium_version, engine_version, read_requests, send, with};
use cef::args::Args;
use cef::{CefString, Settings};
use koyo_browser_ipc::{self as ipc, HostEvent};
use koyo_frame::{End, HELLO_WITHIN};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;

/// Tells the app why its hello was refused, and hands that on.
fn refuse(stream: &mut UnixStream, why: &str) -> String {
    let refused = HostEvent::Error {
        pane: None,
        message: why.to_owned(),
    };
    let _ = ipc::write_frame(stream, &refused);
    why.to_owned()
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .map_err(|error| format!("{name} is not set ({error}); the host is started by Koyo"))
}

/// Says hello on the app's socket: reads the app's, checks it, and keeps the socket for
/// events. The reader's end comes back. A hello refused is refused to the app too, saying why.
fn connect(mut stream: UnixStream, token: &str) -> Result<UnixStream, String> {
    let heard = koyo_frame::hello(
        &mut stream,
        HELLO_WITHIN,
        End::Host,
        ipc::PROTOCOL,
        |stream| Ok(ipc::read_frame::<ipc::Hello>(stream)?.map(|hello| (hello.protocol, hello))),
    );
    let hello = match heard {
        Ok(hello) if koyo_frame::same_token(&hello.token, token) => hello,
        Ok(_) => return Err(refuse(&mut stream, "The browser host refused the connection.")),
        Err(error) => return Err(refuse(&mut stream, &error.to_string())),
    };
    crate::passkeys::enable(hello.passkeys);
    let reader = stream.try_clone().map_err(|e| e.to_string())?;
    let _ = WRITER.set(Mutex::new(stream));
    Ok(reader)
}

/// Starts CEF on `profiles`: as the browser process of its own, or in the app's
/// (`embedded.rs`), where the app runs the loop and CEF is found in the engine.
fn initialize(args: &Args, profiles: &std::path::Path) -> Result<(), String> {
    let mut app = crate::app::HostApp::new();
    std::fs::create_dir_all(profiles).map_err(|e| format!("{}: {e}", profiles.display()))?;
    let profiles = profiles
        .canonicalize()
        .map_err(|e| format!("{}: {e}", profiles.display()))?;
    let generation = super::profiles::generation(&profiles);
    with(|host| {
        host.profiles.clone_from(&profiles);
        host.generation = generation;
    });
    let path = |p: PathBuf| CefString::from(p.to_string_lossy().as_ref());
    let engine = crate::embedded::engine();
    let settings = Settings {
        root_cache_path: path(profiles.clone()),
        cache_path: path(profiles.join("default")),
        persist_session_cookies: 1,
        log_file: path(profiles.join("host.log")),
        log_severity: cef::LogSeverity::WARNING,
        background_color: 0xFFFF_FFFF,
        keychain_service_name: "Koyo Browser Engine".into(),
        main_bundle_path: crate::own_bundle().unwrap_or_default().as_str().into(),
        external_message_pump: i32::from(engine.is_some()),
        framework_dir_path: engine
            .map(|engine| path(crate::embedded::framework(engine)))
            .unwrap_or_default(),
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
    crate::mac_app::install()?;
    let mut app = crate::app::HostApp::new();
    let code = cef::execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        return Err(format!("started as a subprocess ({code})"));
    }
    super::profiles::drop_cleared(&profiles);
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
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        crate::embedded::adopt_app()?;
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
    if crate::embedded::engine().is_some() {
        return;
    }
    cef::quit_message_loop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hello_of_another_protocol_or_token_is_refused_and_the_app_told_why() {
        let hello = |protocol, token: &str| ipc::Hello {
            protocol,
            token: token.into(),
            passkeys: false,
        };
        for given in [hello(ipc::PROTOCOL + 1, "secret"), hello(ipc::PROTOCOL, "secreT")] {
            let (mut app, host) = UnixStream::pair().unwrap();
            ipc::write_frame(&mut app, &given).unwrap();
            let why = connect(host, "secret").unwrap_err();
            let told = ipc::read_frame::<HostEvent>(&mut app).unwrap();
            assert!(
                matches!(&told, Some(HostEvent::Error { pane: None, message }) if *message == why),
                "refused for {why:?}, the app told {told:?}"
            );
        }
    }

    #[test]
    fn an_app_of_another_protocol_is_told_whose_version_is_which() {
        let (mut app, host) = UnixStream::pair().unwrap();
        let hello = ipc::Hello {
            protocol: ipc::PROTOCOL + 1,
            token: "secret".into(),
            passkeys: false,
        };
        ipc::write_frame(&mut app, &hello).unwrap();
        let _ = connect(host, "secret");
        let told = ipc::read_frame::<HostEvent>(&mut app).unwrap();
        let mismatch = koyo_frame::Mismatch {
            app: ipc::PROTOCOL + 1,
            host: ipc::PROTOCOL,
        }
        .to_string();
        assert!(
            matches!(&told, Some(HostEvent::Error { message, .. }) if *message == mismatch),
            "{told:?}"
        );
    }
}
