//! The stdio relay's backend: one control connection to the running app, made on the first
//! app call or subscription and made again when the app went away. Calls share it at once,
//! so a long `browser_wait` never holds up a read. The same connection carries the
//! session's topics; a new connection follows them again before anything else uses it.

use std::collections::BTreeSet;
use std::sync::Arc;

use colosso_app_control::{
    Client, ClientInfo, Command, ControlError, Endpoint, Hello, Notice, Topic,
};
use parking_lot::Mutex;
use serde_json::Value;

use crate::backend::{Backend, Connect, NoticeSink};

/// Connects each MCP session to the app on `endpoint`, as a client named by the session's
/// first call, with the terminal, worktree and working folder this process runs in
/// (`COLOSSO_TERMINAL`, `COLOSSO_WORKTREE`, its cwd). An endpoint error, a socket path too
/// long, is every app call's answer.
pub fn socket_backend(endpoint: Result<Endpoint, ControlError>) -> Connect {
    Arc::new(move |sink| {
        Box::new(Socket {
            endpoint: endpoint.clone(),
            sink,
            state: Mutex::default(),
        })
    })
}

struct Socket {
    endpoint: Result<Endpoint, ControlError>,
    sink: NoticeSink,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The connection, while there is one; a closed one is replaced on the next use.
    client: Option<Arc<Client>>,
    /// Who the session speaks for, from its first call.
    identity: Option<ClientInfo>,
    /// What the session follows, on every connection it makes.
    topics: BTreeSet<Topic>,
}

impl Socket {
    /// The live connection, connecting first when there's none or the app went away. A new
    /// connection follows the session's topics before it's used. The lock is held while
    /// connecting, so calls made meanwhile wait for the one connection rather than each
    /// asking the person to allow the client.
    fn client(
        &self,
        state: &mut State,
        named: Option<&ClientInfo>,
    ) -> Result<Arc<Client>, ControlError> {
        if let Some(client) = state.client.as_ref().filter(|client| !client.is_closed()) {
            return Ok(Arc::clone(client));
        }
        state.client = None;
        if state.identity.is_none() {
            state.identity = named.cloned();
        }
        let Some(identity) = state.identity.clone() else {
            // Nothing has named the session yet, so nothing was followed to follow again.
            return Err(ControlError::app_not_running());
        };
        let endpoint = self.endpoint.clone()?;
        let client = Arc::new(Client::connect(&endpoint, &Hello::from_env(identity))?);
        forward(&client, Arc::clone(&self.sink));
        if !state.topics.is_empty() {
            let topics: Vec<Topic> = state.topics.iter().copied().collect();
            client.subscribe(&topics)?;
        }
        state.client = Some(Arc::clone(&client));
        Ok(client)
    }
}

/// Hands the connection's notices to `sink` on a thread of their own, until the connection
/// ends; [`Notice::Lost`] is the last one.
fn forward(client: &Client, sink: NoticeSink) {
    let notices = client.notices();
    let started = std::thread::Builder::new()
        .name("mcp-app-notices".into())
        .spawn(move || {
            for notice in notices {
                let lost = matches!(notice, Notice::Lost(_));
                sink(notice);
                if lost {
                    return;
                }
            }
        });
    if let Err(error) = started {
        log::warn!("couldn't start reading the app's events: {error}");
    }
}

impl Backend for Socket {
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<&str>,
        command: Command,
    ) -> Result<Value, ControlError> {
        let connection = self.client(&mut self.state.lock(), Some(client))?;
        connection.call_command(&command, request_id)
    }

    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError> {
        let mut state = self.state.lock();
        let connection = self.client(&mut state, Some(client))?;
        let new: Vec<Topic> = (topics.iter().copied())
            .filter(|topic| !state.topics.contains(topic))
            .collect();
        if !new.is_empty() {
            connection.subscribe(&new)?;
            state.topics.extend(new);
        }
        Ok(())
    }

    /// The control protocol has no way to stop following a topic: its events keep coming on
    /// this connection, and the server drops them. The topics aren't followed on the next
    /// connection.
    fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        self.state
            .lock()
            .topics
            .retain(|topic| !topics.contains(topic));
        Ok(())
    }

    fn resubscribe(&self) -> Result<(), ControlError> {
        let mut state = self.state.lock();
        if state.topics.is_empty() {
            return Ok(());
        }
        self.client(&mut state, None).map(drop)
    }
}
