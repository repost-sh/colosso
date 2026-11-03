//! The client against a stand-in host: a socket that answers line by line as the host does,
//! so what the client writes and how it reads replies and events is checked without Bun.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails loudly at the step that broke"
)]

mod support;

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use koyo_agent::client::{self, Client, ClientError};
use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{ClientKind, ThreadParams, ThreadsRead};
use koyo_agent::protocol::wire::{ErrorCode, HOST_VERSION};
use serde_json::json;
use support::{scratch, stand_in};

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
        host.answer_shutdown();
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
            "KOYO_AGENT_PROVIDER",
        ]
        .iter()
        .any(|kept| name == kept)
    }));
}
