//! The client against a stand-in host: a socket that answers line by line as the host does,
//! so what the client writes and how it reads replies and events is checked without Bun.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails loudly at the step that broke"
)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use koyo_agent::client::{self, Client, ClientError};
use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{ClientKind, ThreadParams, ThreadsRead};
use koyo_agent::protocol::wire::{ErrorCode, HOST_VERSION};
use koyo_daemon::DaemonDir;
use serde_json::{Value, json};

/// A host's folder of the test's own, under the checkout's ignored `.superpowers/t`. The path
/// is kept short (the checkout's canonical one, the socket right in it), as a socket's can't
/// pass 104 bytes.
fn scratch() -> DaemonDir {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    let checkout = std::fs::canonicalize(env!("KOYO_CHECKOUT")).unwrap();
    let root = checkout
        .join(".superpowers/t")
        .join(format!("ra{}-{made}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    DaemonDir::new(root, "host")
}

/// One connection to the stand-in host, from its side.
struct Host {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Host {
    fn accept(listener: &UnixListener) -> Host {
        let (stream, _) = listener.accept().unwrap();
        Host {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    /// The next request the client wrote.
    fn request(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'), "a request is a line: {line:?}");
        serde_json::from_str(&line).unwrap()
    }

    fn send(&mut self, line: &Value) {
        writeln!(self.writer, "{line}").unwrap();
    }

    /// Answers the hello the client says first, which says it's a client of `kind`, as a host
    /// of `version`.
    fn greet(&mut self, kind: &str, version: u32) {
        let hello = self.request();
        assert_eq!(hello["method"], "hello");
        assert_eq!(
            hello["params"],
            json!({ "client": { "kind": kind }, "version": HOST_VERSION })
        );
        let result = json!({ "version": version, "pid": 4242, "busy": false });
        self.send(&json!({ "id": hello["id"], "result": result }));
    }
}

/// A stand-in host listening in `dir`, serving one connection with `serve`.
fn stand_in(dir: &DaemonDir, serve: impl FnOnce(Host) + Send + 'static) -> JoinHandle<()> {
    let listener = UnixListener::bind(dir.socket()).unwrap();
    let socket = dir.socket();
    std::thread::spawn(move || {
        serve(Host::accept(&listener));
        drop(listener);
        let _ = std::fs::remove_file(socket);
    })
}

#[test]
fn replies_go_to_their_requests_and_events_to_the_channel_until_the_host_hangs_up() {
    let dir = scratch();
    let host = stand_in(&dir, |mut host| {
        host.greet("test", HOST_VERSION);
        let notice = json!({ "level": "warning", "message": "Claude Code needs signing in." });
        host.send(&json!({ "event": "host.notice", "params": notice }));
        let read = host.request();
        assert_eq!(read["method"], "threads.read");
        let error = json!({ "code": "thread_not_found", "message": "There is no such chat." });
        host.send(&json!({ "id": read["id"], "error": error }));
    });
    let connected =
        Client::over(UnixStream::connect(dir.socket()).unwrap(), ClientKind::Test).unwrap();
    assert_eq!(connected.hello.version, HOST_VERSION);
    assert_eq!(connected.hello.pid, 4242);

    let thread_id = ThreadId("0193a2f0-6c1e-7b3a-9f21-4d5e6f708192".into());
    let refused = connected
        .client
        .request::<ThreadsRead>(&ThreadParams { thread_id });
    assert!(
        matches!(&refused, Err(ClientError::Host(error)) if error.code == ErrorCode::ThreadNotFound),
        "{refused:?}"
    );
    let event = connected
        .events
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(matches!(event, Event::HostNotice(notice) if notice.message.contains("signing in")));

    host.join().unwrap();
    // The host hung up: the channel ends, and so does every request after.
    assert!(
        connected
            .events
            .recv_timeout(Duration::from_secs(5))
            .is_err()
    );
    assert!(connected.client.is_closed());
    let thread_id = ThreadId("0193a2f0-6c1e-7b3a-9f21-4d5e6f708192".into());
    let after = connected
        .client
        .request::<ThreadsRead>(&ThreadParams { thread_id });
    assert!(
        matches!(after, Err(ClientError::Io(_) | ClientError::Closed)),
        "{after:?}"
    );
}

#[test]
fn a_host_of_another_version_is_asked_to_shut_down_and_stops_listening() {
    let dir = scratch();
    let host = stand_in(&dir, |mut host| {
        // The app stops it, so says it is the app.
        host.greet("app", HOST_VERSION + 1);
        let shutdown = host.request();
        assert_eq!(shutdown["method"], "shutdown");
        host.send(&json!({ "id": shutdown["id"], "result": {} }));
    });
    client::stop_host(&dir).unwrap();
    host.join().unwrap();
    assert!(UnixStream::connect(dir.socket()).is_err());
    // Nothing listening is nothing to stop.
    client::stop_host(&dir).unwrap();
}

#[test]
fn the_host_starts_with_only_the_environment_it_needs_in_its_folder() {
    let dir = client::host_dir(Path::new("/data"));
    assert_eq!(dir.socket(), Path::new("/data/agents/host.sock"));
    let command = client::host_command(Path::new("/app/koyo-agent-host"), &dir);
    assert_eq!(command.get_current_dir(), Some(Path::new("/data/agents")));
    let kept: Vec<_> = command
        .get_envs()
        .map(|(name, _)| name.to_owned())
        .collect();
    assert!(kept.iter().all(|name| {
        [
            "HOME",
            "USER",
            "LOGNAME",
            "SHELL",
            "TMPDIR",
            "LANG",
            "KOYO_DATA_DIR",
            "KOYO_LOG",
        ]
        .iter()
        .any(|kept| name == kept)
    }));
}
