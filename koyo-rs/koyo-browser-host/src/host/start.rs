//! The host's start and end: hello on the app's socket, CEF started in a process of the
//! host's own or in the app's (`embedded.rs`), and what stops it.
use super::{chromium_version, engine_version, outgoing, read_requests, send, with};
use crate::blocking::{self, Blocking};
use cef::args::Args;
use cef::{CefString, Settings};
use koyo_browser_ipc::{self as ipc, HostEvent};
use koyo_frame::{End, HELLO_WITHIN};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a host process of its own waits, as it ends, for what it last told the app to be
/// written.
const LAST_WORDS: Duration = Duration::from_secs(2);

/// Tells the app why its hello was refused, and hands that on.
fn refuse(_: Blocking<'_>, stream: &mut UnixStream, why: &str) -> String {
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

/// Says hello on the app's socket: reads the app's, checks it, and has events written to the
/// socket from then on (`outgoing`). The reader's end comes back. A hello refused is refused to
/// the app too, saying why.
fn connect(may: Blocking<'_>, mut stream: UnixStream, token: &str) -> Result<UnixStream, String> {
    let heard = koyo_frame::hello(
        &mut stream,
        HELLO_WITHIN,
        End::Host,
        ipc::PROTOCOL,
        |stream| Ok(ipc::read_frame::<ipc::Hello>(stream)?.map(|hello| (hello.protocol, hello))),
    );
    let hello = match heard {
        Ok(hello) if koyo_frame::same_token(&hello.token, token) => hello,
        Ok(_) => return Err(refuse(may, &mut stream, "The browser host refused the connection.")),
        Err(error) => return Err(refuse(may, &mut stream, &error.to_string())),
    };
    crate::passkeys::enable(hello.passkeys);
    let reader = stream.try_clone().map_err(|e| e.to_string())?;
    outgoing::start(may, stream).map_err(|e| e.to_string())?;
    Ok(reader)
}

/// CEF's root cache, made if it's not there, by the path the file system gives it, and the
/// generation of the profiles in it.
struct Root {
    profiles: PathBuf,
    generation: u32,
}

/// The root cache at `profiles`, readied for CEF: made, and its path and generation read.
fn root(_: Blocking<'_>, profiles: &Path) -> Result<Root, String> {
    std::fs::create_dir_all(profiles).map_err(|e| format!("{}: {e}", profiles.display()))?;
    let profiles = profiles
        .canonicalize()
        .map_err(|e| format!("{}: {e}", profiles.display()))?;
    let generation = super::profiles::generation(&profiles);
    Ok(Root {
        profiles,
        generation,
    })
}

/// Starts CEF on the root cache `root`: as the browser process of its own, or in the app's
/// (`embedded.rs`), where the app runs the loop and CEF is found in the engine. On the main
/// thread.
fn initialize(args: &Args, root: &Root) -> Result<(), String> {
    let mut app = crate::app::HostApp::new();
    let Root {
        profiles,
        generation,
    } = root;
    with(|host| {
        host.profiles.clone_from(profiles);
        host.generation = *generation;
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
        framework: crate::framework::built(),
    });
    blocking::thread("app-socket", move |may| read_requests(may, reader))
        .map(drop)
        .map_err(|e| e.to_string())
}

/// Lets go of what holds CEF's objects: it waits in shutdown for every reference to go.
pub(crate) fn teardown() {
    crate::dialogs::clear();
    crate::menus::clear();
    crate::downloads::clear();
    crate::native::clear();
    with(|host| {
        host.panes.clear();
        host.contexts.clear();
    });
}

/// The browser process's whole life, in a process of its own.
pub(crate) fn run(may: Blocking<'_>, args: &Args) -> Result<(), String> {
    let socket = env(ipc::ENV_SOCKET_FD)?;
    let token = env(ipc::ENV_TOKEN)?;
    let profiles = PathBuf::from(env(ipc::ENV_PROFILE_ROOT)?);
    let stream =
        ipc::inherit::take(&socket).map_err(|e| format!("the app's socket ({socket}): {e}"))?;
    let reader = connect(may, stream, &token)?;
    let ran = run_connected(may, args, &profiles, reader);
    super::profiles::kept();
    outgoing::flush(may, LAST_WORDS);
    ran
}

/// The browser process's life once the app is heard, in a process of its own.
fn run_connected(
    may: Blocking<'_>,
    args: &Args,
    profiles: &Path,
    reader: UnixStream,
) -> Result<(), String> {
    crate::framework::check().map_err(said)?;
    if let Some(engine) = crate::own_bundle() {
        crate::framework::read(may, &crate::embedded::framework(Path::new(&engine)));
    }
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
    super::profiles::drop_cleared(profiles);
    initialize(args, &root(may, profiles)?)?;
    serve(reader)?;
    cef::run_message_loop();
    teardown();
    cef::shutdown();
    Ok(())
}

/// The steps of the host's start in the app's process, each with how long it took.
struct Steps(Vec<(&'static str, Duration)>);

impl Steps {
    fn time<T>(&mut self, step: &'static str, run: impl FnOnce() -> T) -> T {
        let began = Instant::now();
        let ran = run();
        self.0.push((step, began.elapsed()));
        ran
    }
}

/// What of the host's start in the app's process is done off the main thread (`embedded.rs`):
/// the hello heard, CEF loaded from the engine, the root cache readied; and since when it
/// waits for the main thread.
pub(crate) struct Readied {
    reader: UnixStream,
    clock: ipc::client::StartClock,
    root: Root,
    steps: Steps,
    posted: Instant,
}

/// Starts the host in the app's own process up to what the main thread must do, on the thread
/// `embedded::start` runs on, which took `cleared` to clear the old profiles: the same hello as
/// a host process's, then CEF loaded from the engine and its root cache readied. What goes
/// wrong once the app is heard is said on the socket, as a host process would say it.
pub(crate) fn ready_embedded(
    may: Blocking<'_>,
    start: crate::embedded::Start,
    cleared: Duration,
) -> Result<Readied, String> {
    let crate::embedded::Start {
        stream,
        token,
        clock,
        profiles,
        engine,
    } = start;
    let mut steps = Steps(vec![("clear", cleared)]);
    let reader = steps.time("hello", || connect(may, stream, &token))?;
    let readied = (|| {
        steps.time("load", || {
            crate::embedded::load(&engine)?;
            crate::framework::check()?;
            crate::framework::read(may, &crate::embedded::framework(&engine));
            Ok::<_, String>(())
        })?;
        steps.time("root", || root(may, &profiles))
    })();
    let root = readied.map_err(said)?;
    Ok(Readied {
        reader,
        clock,
        root,
        steps,
        posted: Instant::now(),
    })
}

/// The rest of the host's start in the app's own process, on its main thread: CEF started with
/// the app running the loop, then served. Says how long each step of the start took, once.
pub(crate) fn run_embedded(readied: Readied) -> Result<(), String> {
    let Readied {
        reader,
        clock,
        root,
        mut steps,
        posted,
    } = readied;
    let off_main = steps.0.len();
    steps.0.push(("waiting for the main thread", posted.elapsed()));
    let started = (|| {
        steps.time("adopt", crate::embedded::adopt_app)?;
        let args: &'static Args = Box::leak(Box::new(Args::new()));
        steps.time("initialize", || clock.stopped_during(|| initialize(args, &root)))?;
        crate::embedded::pump::started();
        steps.time("serve", || serve(reader))
    })();
    let took = |steps: &[(&str, Duration)]| {
        (steps.iter())
            .map(|(step, took)| format!("{step} {} ms", took.as_millis()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (before, on) = steps.0.split_at(off_main);
    log::info!(
        "the browser engine's start in the app: {} off the main thread; then on it {}",
        took(before),
        took(on)
    );
    started.map_err(said)
}

/// Says `message` on the socket, as a host process would, and hands it on.
fn said(message: String) -> String {
    send(&HostEvent::Error {
        pane: None,
        message: message.clone(),
    });
    message
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
            let why = blocking::in_test(|may| connect(may, host, "secret")).unwrap_err();
            let told = ipc::read_frame::<HostEvent>(&mut app).unwrap();
            assert!(
                matches!(&told, Some(HostEvent::Error { pane: None, message }) if *message == why),
                "refused for {why:?}, the app told {told:?}"
            );
        }
    }

    /// Telling the app something never waits for the app to read it: in Koyo's process the
    /// host runs on the app's main thread, which hands each event to the thread that writes
    /// it. The one test that has events written, as there is one writer in a process.
    #[test]
    fn telling_the_app_never_waits_for_it_to_read() {
        let (mut app, host) = UnixStream::pair().unwrap();
        let hello = ipc::Hello {
            protocol: ipc::PROTOCOL,
            token: "secret".into(),
            passkeys: false,
        };
        ipc::write_frame(&mut app, &hello).unwrap();
        blocking::in_test(|may| connect(may, host, "secret")).unwrap();
        let said = "x".repeat(4 << 20);
        let (told, done) = std::sync::mpsc::channel();
        let telling = said.clone();
        std::thread::spawn(move || {
            for _ in 0..4 {
                send(&HostEvent::Error {
                    pane: None,
                    message: telling.clone(),
                });
            }
            let _ = told.send(());
        });
        let waited = done.recv_timeout(Duration::from_secs(2));
        assert!(waited.is_ok(), "telling the app 16 MiB waited for it to read: {waited:?}");
        let mut heard = 0;
        while heard < 4 {
            if let Some(HostEvent::Error { message, .. }) = ipc::read_frame(&mut app).unwrap()
                && message == said
            {
                heard += 1;
            }
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
        let _ = blocking::in_test(|may| connect(may, host, "secret"));
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
