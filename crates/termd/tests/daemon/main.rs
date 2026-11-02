//! The daemon end to end: a real `colosso-termd` in a folder under `target/`, real
//! programs in real PTYs, clients over its socket. `docs/terminals.md` 13, M1.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_term_control::{
    AttachMode, ClientError, ClientKind, Connection, DaemonCommand, ErrorCode, EventKind, Handle,
    Hello, IfExists, Message, OpenRequest, PROTOCOL, RequestBody, ResponseBody, Selector, Size,
    Status, TermDir, TerminalRecord, TerminalSummary, Welcome,
};
use colosso_term_engine::{Engine, TextFormat};
use crossbeam_channel::Receiver;

mod catalog;
mod catchup;
mod control;
mod http;
mod logs;
mod mcp;
mod pin;
mod send;
mod status;
mod upgrade;

const DAEMON: &str = env!("CARGO_BIN_EXE_colosso-termd");
const WAIT: Duration = Duration::from_secs(20);

/// A fresh folder for one test's daemon, under the checkout; short, since a unix socket's
/// path is limited to about a hundred bytes.
fn test_dir(name: &str) -> TermDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tt")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    TermDir::at(std::fs::canonicalize(&root).unwrap().join("d"))
}

fn hello(kind: ClientKind, cwd: Option<PathBuf>) -> Hello {
    Hello {
        capabilities: vec!["answers_queries".into()],
        cwd,
        ..Hello::new(kind, "test", "0")
    }
}

struct Client {
    connection: Arc<Connection>,
    inbox: Receiver<Message>,
    welcome: Welcome,
}

/// Shuts a test's daemon down when the test ends, passed or failed, so none is left running.
struct StopOnDrop(TermDir);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = colosso_term_control::client::stop_daemon(&self.0, &hello(ClientKind::Cli, None));
    }
}

fn connect(dir: &TermDir, kind: ClientKind) -> Client {
    connect_from(dir, kind, None)
}

/// Connects as a client running in `cwd`, which the daemon takes `current` to mean.
fn connect_from(dir: &TermDir, kind: ClientKind, cwd: Option<PathBuf>) -> Client {
    let (tx, inbox) = colosso_term_control::client::inbox();
    let (connection, welcome) =
        Connection::connect_or_start(dir, &DaemonCommand::binary(DAEMON), &hello(kind, cwd), tx)
            .unwrap();
    Client {
        connection,
        inbox,
        welcome,
    }
}

impl Client {
    fn open(
        &self,
        name: &str,
        cwd: &Path,
        command: &[&str],
        respawn: bool,
    ) -> (TerminalRecord, bool) {
        let request = OpenRequest {
            worktree: None,
            name: name.into(),
            command: command.iter().map(|s| (*s).to_owned()).collect(),
            cwd: Some(cwd.to_path_buf()),
            env: BTreeMap::new(),
            size: Some(Size::cells(60, 12)),
            labels: BTreeSet::new(),
            if_exists: IfExists::Attach,
            respawn,
        };
        match self.connection.request(RequestBody::Open(request)).unwrap() {
            ResponseBody::Opened { terminal, existed } => (*terminal, existed),
            other => panic!("{other:?}"),
        }
    }

    fn attach(&self, handle: &Handle, since: Option<u64>) -> (u32, u64) {
        let request = RequestBody::Attach {
            handle: handle.clone(),
            size: Some(Size::cells(60, 12)),
            mode: AttachMode::Control,
            since_offset: since,
        };
        match self.connection.request(request).unwrap() {
            ResponseBody::Attached { index, offset, .. } => (index, offset),
            other => panic!("{other:?}"),
        }
    }

    /// Attaches to a program that prints `ready`, and waits for it: in the snapshot when
    /// the program was quicker than the attach, else in the output after it.
    fn attach_ready(&self, handle: &Handle) -> u32 {
        let (index, _) = self.attach(handle, None);
        let (_, screen) = self.snapshot_text(index);
        if !screen.contains("ready") {
            self.output_until(index, "ready");
        }
        index
    }

    /// Output on `index` until the text seen so far contains `needle`; the text and the
    /// offset just past the last byte received.
    fn output_until(&self, index: u32, needle: &str) -> (String, u64) {
        let mut text = String::new();
        let mut end = 0;
        let deadline = Instant::now() + WAIT;
        while !text.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.inbox.recv_timeout(left) {
                Ok(Message::Output(output)) if output.index == index => {
                    end = output.offset + output.bytes.len() as u64;
                    text.push_str(&String::from_utf8_lossy(&output.bytes));
                }
                Ok(_) => {}
                Err(timeout) => panic!("no {needle:?} within {WAIT:?} ({timeout}); saw {text:?}"),
            }
        }
        (text, end)
    }

    /// The next snapshot on `index`, decoded to the screen's text.
    fn snapshot_text(&self, index: u32) -> (u64, String) {
        let mut bytes = Vec::new();
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.inbox.recv_timeout(left) {
                Ok(Message::Snapshot(chunk)) if chunk.index == index => {
                    bytes.extend_from_slice(&chunk.bytes);
                    if chunk.chunk + 1 == chunk.of {
                        let engine = Engine::restore(&bytes).unwrap_or_else(|error| {
                            panic!(
                                "{error:?} restoring {} bytes starting {:?}",
                                bytes.len(),
                                &bytes[..bytes.len().min(8)]
                            )
                        });
                        return (chunk.offset, engine.screen_text(TextFormat::Plain).unwrap());
                    }
                }
                Ok(Message::Output(_)) => panic!("output before the snapshot"),
                Ok(_) => {}
                Err(timeout) => panic!("no snapshot within {WAIT:?} ({timeout})"),
            }
        }
    }

    fn event_until(&self, handle: &Handle, matches: impl Fn(&EventKind) -> bool) -> EventKind {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.inbox.recv_timeout(left) {
                Ok(Message::Event(event)) if event.handle == *handle && matches(&event.kind) => {
                    return event.kind;
                }
                Ok(_) => {}
                Err(timeout) => panic!("no such event within {WAIT:?} ({timeout})"),
            }
        }
    }

    fn list(&self) -> Vec<TerminalSummary> {
        match self
            .connection
            .request(RequestBody::ListTerminals {
                worktree: None,
                status: None,
            })
            .unwrap()
        {
            ResponseBody::Terminals { terminals } => terminals,
            other => panic!("{other:?}"),
        }
    }
}

/// A program that announces itself and echoes lines back, through sh.
const ECHOER: [&str; 3] = [
    "/bin/sh",
    "-c",
    "printf 'ready\\n'; while read line; do printf 'got:%s\\n' \"$line\"; done",
];

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "{what} didn't happen within {WAIT:?}"
        );
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

#[test]
fn a_terminal_outlives_its_client_and_reattaches_from_a_snapshot() {
    let dir = test_dir("outlives");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let a = connect(&dir, ClientKind::Cli);
    assert_eq!(a.welcome.protocol, PROTOCOL);
    let (terminal, existed) = a.open("echoer", &cwd, &ECHOER, false);
    assert!(!existed);
    assert_eq!(terminal.name, "echoer");
    // target/ is inside this checkout, which the daemon found without being told.
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert_eq!(
        terminal
            .worktree
            .as_ref()
            .map(|worktree| worktree.path.as_path()),
        Some(checkout.as_path())
    );
    assert!(terminal.pid.is_some());
    let handle = terminal.handle;

    let (index, offset) = a.attach(&handle, None);
    let (snapshot_offset, screen) = a.snapshot_text(index);
    assert_eq!(snapshot_offset, offset);
    // A program quicker than the attach printed into the snapshot.
    if !screen.contains("ready") {
        a.output_until(index, "ready");
    }
    a.connection.input(index, b"abc\r").unwrap();
    let (_, reached) = a.output_until(index, "got:abc");
    a.connection.close();
    drop(a);

    // The program is still there for the next client.
    let b = connect(&dir, ClientKind::App);
    wait_until("the first client to be forgotten", || {
        b.list().iter().all(|t| t.record.attached_clients == 0)
    });
    let listed = b.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].record.handle, handle);
    assert!(listed[0].record.pid.is_some());
    let (index_b, offset_b) = b.attach(&handle, None);
    assert!(offset_b >= reached);
    let (_, screen) = b.snapshot_text(index_b);
    assert!(screen.contains("ready"), "{screen:?}");
    assert!(screen.contains("got:abc"), "{screen:?}");

    // Two clients see the same output.
    let c = connect(&dir, ClientKind::Cli);
    let (index_c, _) = c.attach(&handle, None);
    c.snapshot_text(index_c);
    b.connection.input(index_b, b"xyz\r").unwrap();
    b.output_until(index_b, "got:xyz");
    c.output_until(index_c, "got:xyz");

    // A client that holds state up to an offset resumes from the ring without a snapshot.
    b.connection
        .request(RequestBody::Detach {
            handle: handle.clone(),
        })
        .unwrap();
    let (index_b2, offset_b2) = b.attach(&handle, Some(reached));
    assert_eq!(offset_b2, reached);
    let (replayed, _) = b.output_until(index_b2, "got:xyz");
    assert!(
        !replayed.contains("got:abc"),
        "replay started at {reached}, not from the beginning: {replayed:?}"
    );

    // Closing ends the program and forgets the terminal.
    b.connection
        .request(RequestBody::Close {
            handle: handle.clone(),
            force: true,
        })
        .unwrap();
    c.event_until(&handle, |kind| matches!(kind, EventKind::Closed));
    assert!(b.list().is_empty());
    b.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());
}

#[test]
fn the_registry_survives_a_restart_and_respawns() {
    let dir = test_dir("restart");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let a = connect(&dir, ClientKind::Cli);
    let (terminal, _) = a.open(
        "sleeper",
        &cwd,
        &["/bin/sh", "-c", "printf 'up\\n'; sleep 30"],
        false,
    );
    let handle = terminal.handle;
    let (index, _) = a.attach(&handle, None);
    if !a.snapshot_text(index).1.contains("up") {
        a.output_until(index, "up");
    }
    a.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());

    let b = connect(&dir, ClientKind::Cli);
    let listed = b.list();
    assert_eq!(listed.len(), 1);
    let remembered = &listed[0].record;
    assert_eq!(remembered.handle, handle);
    assert_eq!(remembered.status, Status::Exited);
    assert!(remembered.exit.is_some_and(|exit| exit.daemon_restart));
    assert_eq!(remembered.pid, None);
    let request = RequestBody::Attach {
        handle: handle.clone(),
        size: None,
        mode: AttachMode::Observe,
        since_offset: None,
    };
    let error = b.connection.request(request).unwrap_err();
    assert!(
        matches!(error, ClientError::Protocol(ref e) if e.code == ErrorCode::Exited),
        "{error:?}"
    );

    // Its last screen reads as it was, kept across the restart.
    let read = RequestBody::ReadScreen {
        handle: handle.clone(),
    };
    let ResponseBody::Screen(kept) = b.connection.request(read).unwrap() else {
        panic!("not a screen");
    };
    assert!(
        kept.lines.iter().any(|line| line == "up"),
        "{:?}",
        kept.lines
    );

    let (again, existed) = b.open("sleeper", &cwd, &[], true);
    assert!(existed);
    assert_eq!(again.handle, handle);
    assert_eq!(again.incarnation, 2);
    assert!(again.pid.is_some());
    // Run again, it starts from the screen it had, then its program prints anew.
    let (index, _) = b.attach(&handle, None);
    let (_, screen) = b.snapshot_text(index);
    assert!(
        screen.contains("[restored after the terminal daemon restarted]"),
        "{screen}"
    );
    if screen
        .lines()
        .filter(|line| line.trim_end() == "up")
        .count()
        < 2
    {
        b.output_until(index, "up");
    }
    b.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());
}

#[test]
fn worktree_selectors_scope_the_list() {
    let dir = test_dir("scoped");
    let _stop = StopOnDrop(dir.clone());
    let root = dir.root().parent().unwrap().to_path_buf();
    // Two fake checkouts, as git lays them out.
    let main = root.join("repo");
    let feat = root.join("repo-feat");
    std::fs::create_dir_all(main.join(".git/worktrees/feat")).unwrap();
    std::fs::create_dir_all(&feat).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        main.join(".git/worktrees/feat/HEAD"),
        "ref: refs/heads/feat-x\n",
    )
    .unwrap();
    std::fs::write(
        main.join(".git/worktrees/feat/gitdir"),
        format!("{}/.git\n", feat.display()),
    )
    .unwrap();
    std::fs::write(
        feat.join(".git"),
        format!("gitdir: {}/.git/worktrees/feat\n", main.display()),
    )
    .unwrap();

    let a = connect(&dir, ClientKind::Cli);
    let (in_main, _) = a.open("shell", &main, &["/bin/sh", "-c", "sleep 30"], false);
    let (in_feat, _) = a.open("shell", &feat, &["/bin/sh", "-c", "sleep 30"], false);
    assert_ne!(
        in_main.handle, in_feat.handle,
        "the same name in two worktrees is two terminals"
    );
    assert_eq!(
        in_main.worktree.as_ref().unwrap().branch.as_deref(),
        Some("main")
    );
    assert_eq!(
        in_feat.worktree.as_ref().unwrap().branch.as_deref(),
        Some("feat-x")
    );
    assert_eq!(
        in_feat.worktree.as_ref().unwrap().repo_main,
        in_main.worktree.as_ref().unwrap().path
    );

    let request = RequestBody::ListTerminals {
        worktree: Some(Selector::Branch("feat-x".into())),
        status: None,
    };
    let ResponseBody::Terminals { terminals } = a.connection.request(request).unwrap() else {
        panic!()
    };
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].record.handle, in_feat.handle);
    assert_eq!(a.list().len(), 2);

    let (same, existed) = a.open("shell", &feat, &[], false);
    assert!(existed);
    assert_eq!(same.handle, in_feat.handle);
    a.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());
}
