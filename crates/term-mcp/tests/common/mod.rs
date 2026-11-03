//! What the end-to-end tests share: a real `rmcp` client over an in-memory duplex to the
//! server, and a fake Koyo on a real control socket in the checkout's `target/tmp`. The
//! fake app welcomes every client, answers calls as the test says, reports what it was sent,
//! and pushes topic events to the connections following them. Nothing here starts the real
//! app or the terminal daemon. Each test file declares it as `#[cfg(test)] pub mod common;`,
//! so the lints' leave for tests reaches it and the helpers a file leaves unused aren't dead
//! code.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use koyo_app_control::frame::{
    Call, ClientFrame, MAX_CLIENT_FRAME, MAX_SERVER_FRAME, ServerFrame,
};
use koyo_app_control::transport::{Connection, FrameWriter, Listener};
use koyo_app_control::{ControlError, Endpoint, Hello, Scope as AppScope, Topic};
use koyo_term_control::catalog::Scope;
use koyo_term_control::{DaemonCommand, TermDir};
use koyo_term_mcp::{Config, Launcher, Server, serve, socket_backend};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, ClientRequest,
    Implementation, ProtocolVersion, Request, ResourceUpdatedNotificationParam,
    SubscribeRequestParams, UnsubscribeRequestParams,
};
use rmcp::service::{ClientServiceExt as _, NotificationContext, RoleClient, RunningService};
use rmcp::{ClientHandler, ClientLifecycleMode};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// How long a test waits for something the fake app or the client should hear.
pub const HEARING: Duration = Duration::from_secs(20);

/// The MCP client's name in `initialize`.
pub const CLIENT: &str = "e2e-client";

/// A fresh, empty folder in the checkout's `target/tmp`, named short so socket paths in it
/// stay well under the limit. Not `CARGO_TARGET_TMPDIR`, which a shared build folder may put
/// outside the checkout.
///
/// # Panics
/// When the folder can't be made.
pub fn scratch(name: &str) -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();
    let dir = workspace.join("target/tmp").join(format!("mcp-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The control socket in `dir`.
pub fn endpoint(dir: &Path) -> Endpoint {
    Endpoint::at(dir.join("app.sock"))
}

/// The server's terminal side, on a daemon folder in `dir` and a daemon that can't start,
/// so a test that reached for the terminals would fail rather than start one.
pub fn config(dir: &Path) -> Config {
    Config {
        dir: TermDir::at(dir.join("term")),
        daemon: DaemonCommand {
            program: dir.join("no-daemon"),
            args: Vec::new(),
        },
        grants: Scope::AGENT.to_vec(),
        cwd: None,
        session: None,
        client: None,
    }
}

/// The stdio relay's server on `endpoint`, with `launcher` answering `app_launch`.
pub fn relay(dir: &Path, endpoint: &Endpoint, launcher: Option<Launcher>) -> Server {
    Server::new(config(dir), &socket_backend(Ok(endpoint.clone())), launcher)
}

/// How the fake app answers a call.
pub type Answer = Arc<dyn Fn(&Call) -> Result<Value, ControlError> + Send + Sync>;

/// What the fake app was sent.
#[derive(Debug, PartialEq)]
pub enum Heard {
    Hello(Hello),
    Call(Call),
    Subscribe(Vec<Topic>),
}

/// One welcomed connection: its writing end and the topics it follows.
struct Peer {
    writer: FrameWriter,
    topics: Mutex<BTreeSet<Topic>>,
}

/// A fake Koyo listening on a control socket until stopped or dropped.
pub struct FakeApp {
    endpoint: Endpoint,
    peers: Arc<Mutex<Vec<Arc<Peer>>>>,
    stopped: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
    heard: Receiver<Heard>,
}

impl FakeApp {
    /// Listens on `endpoint`, welcoming every client with the read and navigate scopes and
    /// answering its calls with `answer`.
    ///
    /// # Panics
    /// When the socket can't be bound.
    pub fn start(endpoint: &Endpoint, answer: Answer) -> FakeApp {
        let listener = Listener::bind(endpoint).unwrap();
        let (tell, heard) = crossbeam_channel::unbounded();
        let peers: Arc<Mutex<Vec<Arc<Peer>>>> = Arc::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let accepting = {
            let (peers, stopped) = (Arc::clone(&peers), Arc::clone(&stopped));
            std::thread::spawn(move || {
                while let Ok(connection) = listener.accept() {
                    if stopped.load(Ordering::SeqCst) {
                        break;
                    }
                    let (peers, tell, answer) = (Arc::clone(&peers), tell.clone(), answer.clone());
                    std::thread::spawn(move || serve_peer(connection, &peers, &tell, &answer));
                }
            })
        };
        FakeApp {
            endpoint: endpoint.clone(),
            peers,
            stopped,
            accepting: Some(accepting),
            heard,
        }
    }

    /// What the app is sent next.
    ///
    /// # Panics
    /// When nothing comes within [`HEARING`].
    pub fn next(&self) -> Heard {
        self.heard.recv_timeout(HEARING).unwrap()
    }

    /// The next call the app is sent, after the hello when there is one.
    ///
    /// # Panics
    /// When nothing comes within [`HEARING`], or something else does.
    pub fn next_call(&self) -> Call {
        loop {
            match self.next() {
                Heard::Call(call) => return call,
                Heard::Hello(_) => {}
                Heard::Subscribe(topics) => {
                    panic!("expected a call, heard a subscribe to {topics:?}")
                }
            }
        }
    }

    /// The topics of the next subscribe the app is sent, after the hello when there is one.
    ///
    /// # Panics
    /// When nothing comes within [`HEARING`], or something else does.
    pub fn next_subscribe(&self) -> Vec<Topic> {
        loop {
            match self.next() {
                Heard::Subscribe(topics) => return topics,
                Heard::Hello(_) => {}
                Heard::Call(call) => panic!("expected a subscribe, heard {call:?}"),
            }
        }
    }

    /// Tells every connection following `topic` that it changed.
    pub fn push(&self, topic: Topic) {
        for peer in self.peers.lock().iter() {
            if peer.topics.lock().contains(&topic) {
                let _ = peer
                    .writer
                    .write(&ServerFrame::Event { topic }, MAX_SERVER_FRAME);
            }
        }
    }

    /// Stops listening and hangs up every connection, as a quitting app does.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        let Some(accepting) = self.accepting.take() else {
            return;
        };
        self.stopped.store(true, Ordering::SeqCst);
        // A connection wakes the listener, which then sees it should stop and lets the
        // socket go.
        let _ = Connection::connect(&self.endpoint);
        accepting.join().unwrap();
        for peer in self.peers.lock().drain(..) {
            peer.writer.shutdown();
        }
    }
}

impl Drop for FakeApp {
    fn drop(&mut self) {
        self.halt();
    }
}

fn serve_peer(
    connection: Connection,
    peers: &Mutex<Vec<Arc<Peer>>>,
    tell: &Sender<Heard>,
    answer: &Answer,
) {
    let (mut reader, writer) = connection.split().unwrap();
    // A connection that only checks the app answers says nothing.
    let Ok(Some(ClientFrame::Hello(hello))) = reader.read::<ClientFrame>(MAX_CLIENT_FRAME) else {
        return;
    };
    let _ = tell.send(Heard::Hello(hello));
    let grant = vec![AppScope::Read, AppScope::Navigate];
    let welcome = ServerFrame::Welcome { grant };
    if writer.write(&welcome, MAX_SERVER_FRAME).is_err() {
        return;
    }
    let peer = Arc::new(Peer {
        writer: writer.clone(),
        topics: Mutex::default(),
    });
    peers.lock().push(Arc::clone(&peer));
    while let Ok(Some(frame)) = reader.read::<ClientFrame>(MAX_CLIENT_FRAME) {
        match frame {
            ClientFrame::Call(call) => {
                let result = answer(&call);
                let id = call.id;
                let _ = tell.send(Heard::Call(call));
                let _ = writer.write(&ServerFrame::answer(id, result), MAX_SERVER_FRAME);
            }
            ClientFrame::Subscribe { topics } => {
                peer.topics.lock().extend(topics.iter().copied());
                let _ = tell.send(Heard::Subscribe(topics));
            }
            ClientFrame::Hello(_) => break,
        }
    }
}

/// An MCP client that hands over the URIs of `notifications/resources/updated`.
pub struct Recorder {
    config: ClientConfig,
    updates: UnboundedSender<String>,
}

impl ClientHandler for Recorder {
    fn get_info(&self) -> ClientConfig {
        self.config.clone()
    }

    async fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        let _ = self.updates.send(params.uri);
    }
}

pub type Session = RunningService<RoleClient, Recorder>;

/// A client named [`CLIENT`] on `version` (`initialize` up to 2025-11-25, `server/discover`
/// from 2026-07-28 on) in a session with `server`, and the resource updates it hears.
///
/// # Panics
/// When the session doesn't start.
pub async fn connect(
    server: Server,
    version: ProtocolVersion,
) -> (Session, UnboundedReceiver<String>) {
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move { serve(server, server_io).await });
    let (updates, heard) = unbounded_channel();
    let config = ClientConfig::new(
        ClientCapabilities::default(),
        Implementation::new(CLIENT, "9.9"),
    )
    .with_protocol_version(version.clone());
    let lifecycle = if version.has_initialize() {
        ClientLifecycleMode::Initialize
    } else {
        ClientLifecycleMode::Discover {
            preferred_versions: vec![version],
        }
    };
    let session = Recorder { config, updates }
        .serve_with_lifecycle(client_io, lifecycle)
        .await
        .unwrap();
    (session, heard)
}

/// The next thing `heard` hears.
///
/// # Panics
/// When nothing comes within [`HEARING`].
pub async fn next<T>(heard: &mut UnboundedReceiver<T>) -> T {
    tokio::time::timeout(HEARING, heard.recv())
        .await
        .unwrap()
        .unwrap()
}

/// Calls tool `name` with `arguments`, an object.
///
/// # Panics
/// When `arguments` isn't an object or the call can't be made.
pub async fn call(session: &Session, name: &str, arguments: Value) -> CallToolResult {
    let Value::Object(arguments) = arguments else {
        panic!("arguments are an object");
    };
    let request = CallToolRequestParams::new(name.to_owned()).with_arguments(arguments);
    session.peer().call_tool(request).await.unwrap()
}

/// The text blocks of `result`.
pub fn texts(result: &CallToolResult) -> Vec<String> {
    (result.content.iter())
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

/// The structured error of a failed result.
///
/// # Panics
/// When the result isn't an error with structured content.
pub fn failure(result: &CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(true), "{result:?}");
    result.structured_content.clone().unwrap()
}

/// `resources/subscribe`, or `resources/unsubscribe` when not `on`.
///
/// # Panics
/// When the server refuses.
pub async fn legacy(session: &Session, uri: &str, on: bool) {
    let request = if on {
        ClientRequest::SubscribeRequest(Request::new(SubscribeRequestParams::new(uri)))
    } else {
        ClientRequest::UnsubscribeRequest(Request::new(UnsubscribeRequestParams::new(uri)))
    };
    session.peer().send_request(request).await.unwrap();
}
