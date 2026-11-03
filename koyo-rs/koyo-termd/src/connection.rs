//! One client's connection: the handshake, then its frames read and dispatched for as long
//! as it lasts; and the `Client` the rest of the daemon addresses it by.

use std::collections::HashMap;
use std::io::{self, Read as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use koyo_term_control::{
    Actor, ActorKind, ClientInfo, ClientKind, DAEMON_VERSION, Decoder, ErrorCode, Event, EventKind,
    Handle, Hello, Message, Outcome, PROTOCOL, ProtocolError, Response, ResponseBody,
    SNAPSHOT_FORMAT, Welcome,
};
use parking_lot::Mutex;

use crate::outbound::{Lane, Outbound};
use crate::server::Daemon;

/// A client that said hello.
pub struct Client {
    pub id: u32,
    pub actor: Actor,
    pub info: ClientInfo,
    pub cwd: Option<PathBuf>,
    pub capabilities: Vec<String>,
    pub outbound: Arc<Outbound>,
    attachments: Mutex<Attachments>,
}

#[derive(Default)]
struct Attachments {
    next_index: u32,
    by_index: HashMap<u32, Handle>,
    by_handle: HashMap<Handle, (u32, Arc<Lane>)>,
}

impl Client {
    /// Queues a control message; what fails to encode is logged, since nothing on the wire
    /// could carry the failure.
    pub fn send(&self, message: &Message) {
        let mut out = Vec::new();
        match message.encode(&mut out) {
            Ok(()) => self.outbound.send(Bytes::from(out)),
            Err(error) => log::error!(
                "client {}: couldn't encode a {:?}: {error}",
                self.id,
                message.kind()
            ),
        }
    }

    pub fn respond(&self, req: u32, outcome: impl Into<Outcome>) {
        self.send(&Message::Response(Response {
            req,
            outcome: outcome.into(),
        }));
    }

    pub fn event(&self, handle: &Handle, kind: EventKind) {
        self.send(&Message::Event(Event {
            handle: handle.clone(),
            kind,
        }));
    }

    /// The index this connection's hot frames use for `handle`, allotted on first attach.
    pub fn attach(&self, handle: &Handle) -> (u32, Arc<Lane>) {
        let mut attachments = self.attachments.lock();
        if let Some((index, lane)) = attachments.by_handle.get(handle) {
            return (*index, Arc::clone(lane));
        }
        let index = attachments.next_index;
        attachments.next_index += 1;
        let lane = Arc::new(Lane::default());
        attachments.by_index.insert(index, handle.clone());
        attachments
            .by_handle
            .insert(handle.clone(), (index, Arc::clone(&lane)));
        (index, lane)
    }

    pub fn detach(&self, handle: &Handle) {
        let mut attachments = self.attachments.lock();
        if let Some((index, _)) = attachments.by_handle.remove(handle) {
            attachments.by_index.remove(&index);
        }
    }

    pub fn handle_at(&self, index: u32) -> Option<Handle> {
        self.attachments.lock().by_index.get(&index).cloned()
    }

    pub fn attached(&self) -> Vec<Handle> {
        self.attachments.lock().by_handle.keys().cloned().collect()
    }

    /// Whether this client runs an engine that answers the terminal's queries itself.
    pub fn answers_queries(&self) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == "answers_queries")
    }
}

/// Runs a connection to its end; on the connection's own thread.
pub fn serve(daemon: &Arc<Daemon>, mut stream: UnixStream, id: u32) {
    let client = match handshake(daemon, &mut stream, id) {
        Ok(client) => client,
        Err(error) => {
            log::info!("connection {id} ended before it said hello: {error}");
            return;
        }
    };
    log::info!(
        "client {id} is {} ({} {} pid {})",
        client.actor.id,
        client.info.name,
        client.info.version,
        client.info.pid
    );
    daemon.add_client(Arc::clone(&client));
    let mut decoder = Decoder::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                log::debug!("client {id}: read ended: {error}");
                break;
            }
        };
        decoder.feed(&buf[..n]);
        loop {
            match decoder.next_frame() {
                Ok(Some(frame)) => match Message::decode(frame) {
                    Ok(Message::Request(request)) => daemon.request(&client, request),
                    Ok(Message::Input(input)) => daemon.input(&client, input),
                    Ok(other) => log::warn!(
                        "client {id} sent a {:?} frame, which only the daemon sends",
                        other.kind()
                    ),
                    Err(error) => {
                        log::warn!("client {id} sent a frame the daemon can't read: {error}");
                    }
                },
                Ok(None) => break,
                Err(error) => {
                    log::warn!("client {id}: closing, its stream is out of step: {error}");
                    client.outbound.close();
                    daemon.remove_client(&client);
                    return;
                }
            }
        }
        if client.outbound.is_closed() {
            break;
        }
    }
    client.outbound.close();
    daemon.remove_client(&client);
    log::info!("client {id} left");
}

/// Reads the `Hello`, answers `Welcome`, and makes the client.
fn handshake(daemon: &Arc<Daemon>, stream: &mut UnixStream, id: u32) -> io::Result<Arc<Client>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let hello = read_hello(stream)?;
    stream.set_read_timeout(None)?;
    let writer = stream.try_clone()?;
    let outbound = Outbound::start(writer, &id.to_string())?;
    if hello.protocol != PROTOCOL {
        let error = ProtocolError::new(
            ErrorCode::DaemonVersionMismatch,
            format!(
                "this daemon speaks protocol {PROTOCOL}, the client protocol {}",
                hello.protocol
            ),
        );
        let mut out = Vec::new();
        if Message::Response(Response {
            req: 0,
            outcome: Outcome::Err(error),
        })
        .encode(&mut out)
        .is_ok()
        {
            outbound.send(Bytes::from(out));
        }
        // Written before the hang-up, so the client learns why.
        outbound.close_after(std::time::Instant::now() + Duration::from_secs(1));
        return Err(io::Error::other(format!(
            "client speaks protocol {}",
            hello.protocol
        )));
    }
    let actor = actor_for(&hello.client, id);
    let client = Arc::new(Client {
        id,
        actor: actor.clone(),
        info: hello.client,
        cwd: hello.cwd,
        capabilities: hello.capabilities,
        outbound,
        attachments: Mutex::default(),
    });
    client.send(&Message::Welcome(Welcome {
        protocol: PROTOCOL,
        daemon_version: DAEMON_VERSION.to_string(),
        engine_commit: koyo_term_engine::GHOSTTY_COMMIT.to_owned(),
        snapshot_format: SNAPSHOT_FORMAT,
        client_id: id,
        actor,
        features: daemon.features(),
    }));
    Ok(client)
}

fn read_hello(stream: &mut UnixStream) -> io::Result<Hello> {
    let mut decoder = Decoder::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "closed before hello",
            ));
        }
        decoder.feed(&buf[..n]);
        if let Some(frame) = decoder.next_frame().map_err(io::Error::other)? {
            return match Message::decode(frame).map_err(io::Error::other)? {
                Message::Hello(hello) => Ok(hello),
                other => Err(io::Error::other(format!(
                    "first frame was a {:?}, not a hello",
                    other.kind()
                ))),
            };
        }
    }
}

/// Who a client is: the app's user and a command line are the person; a relay is the agent
/// behind it, named by its client name and this connection.
fn actor_for(info: &ClientInfo, id: u32) -> Actor {
    match info.kind {
        ClientKind::App | ClientKind::Mobile | ClientKind::Cli => Actor {
            kind: ActorKind::Human,
            id: "human".into(),
            label: "you".into(),
        },
        ClientKind::Relay => Actor {
            kind: ActorKind::Agent,
            id: format!("{}:{id:x}", info.name),
            label: info.name.clone(),
        },
    }
}

/// A quick way to say a request succeeded with nothing to add.
pub fn done() -> Outcome {
    Outcome::Ok(ResponseBody::Done)
}
