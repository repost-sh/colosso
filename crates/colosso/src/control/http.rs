//! The MCP server the app serves over Streamable HTTP while the person has it on (Settings →
//! MCP server): `http://127.0.0.1:<port>/mcp`, on the policy's port. Off, nothing listens.
//! Each MCP session reaches app control in-process ([`InProcess`]), on the same channel as
//! the socket's connections, so approval, scopes, confirmations, receipts and the log apply
//! unchanged; its terminal tools reach the daemon as the session's client.

use std::sync::{Arc, Mutex};

use colosso_app_control::{ClientInfo, Command, ControlError, Notice, Topic};
use colosso_term_control::TermDir;
use colosso_term_control::catalog::Scope as TerminalScope;
use colosso_term_mcp::http::HttpServer;
use colosso_term_mcp::{Backend, Config, Connect, NoticeSink};
use futures::channel::mpsc::UnboundedSender;
use gpui_kit::App;
use serde_json::Value;

use super::in_process::InProcess;
use super::{Control, Request, policy};
use crate::terminals;

/// The running server, and the port it was started on.
pub(crate) struct Serving {
    port: u16,
    server: HttpServer,
}

/// What Settings and `app_status` say about the server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpState {
    /// Its URL, while it serves.
    pub url: Option<String>,
    /// Why it couldn't serve the policy's port, while it's on.
    pub error: Option<String>,
}

/// The server's state now.
pub fn state(cx: &App) -> HttpState {
    let Some(control) = cx.try_global::<Control>() else {
        return HttpState::default();
    };
    HttpState {
        url: control.http.as_ref().map(|serving| serving.server.url()),
        error: control.http_error.clone(),
    }
}

/// Serves MCP over HTTP while the policy has it on, on its port, and stops otherwise. A port
/// that can't be bound is kept as the error Settings shows; the next change tries again.
pub(crate) fn apply(cx: &mut App) {
    let policy = policy(cx);
    let Some(control) = cx.try_global::<Control>() else {
        return;
    };
    let serving = control.http.as_ref().map(|serving| serving.port);
    if !policy.enabled {
        if serving.is_some() || control.http_error.is_some() {
            let control = Control::get_mut(cx);
            control.http = None;
            control.http_error = None;
            log::info!("the MCP server stopped");
        }
        return;
    }
    if serving == Some(policy.port) {
        return;
    }
    let requests = control.requests.clone();
    // The old server lets the port go before a new one binds, which may be the same port.
    Control::get_mut(cx).http = None;
    let started = HttpServer::start(policy.port, terminals_config(), connect(requests));
    let control = Control::get_mut(cx);
    match started {
        Ok(server) => {
            log::info!("the MCP server serves {}", server.url());
            control.http = Some(Serving {
                port: policy.port,
                server,
            });
            control.http_error = None;
        }
        Err(error) => {
            let message = bind_error(policy.port, &error);
            log::warn!("{message}");
            control.http_error = Some(message);
        }
    }
}

/// Why `port` couldn't be served, as Settings says it.
fn bind_error(port: u16, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("Port {port} is in use by another app. Choose another port.")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("Colosso may not listen on port {port}. Choose another port.")
        }
        _ => format!("Colosso can't listen on port {port}: {error}"),
    }
}

/// How an HTTP session reaches the terminals: the daemon of this data folder, started from
/// this executable when none runs, with what an agent may do there by default.
fn terminals_config() -> Config {
    Config {
        dir: terminals::term_dir()
            .unwrap_or_else(|| TermDir::at(std::env::temp_dir().join("colosso-term"))),
        daemon: terminals::daemon_command(),
        grants: TerminalScope::AGENT.to_vec(),
        cwd: None,
        session: None,
        client: None,
    }
}

/// Makes each MCP session's backend: an in-process client of app control.
fn connect(requests: UnboundedSender<Request>) -> Connect {
    Arc::new(move |sink: NoticeSink| -> Box<dyn Backend> {
        Box::new(Session {
            client: InProcess::from_requests(requests.clone()),
            welcomed: Mutex::new(None),
            followed: Mutex::new(Vec::new()),
            sink,
        })
    })
}

/// One MCP session's way into app control.
struct Session {
    client: InProcess,
    /// The client it said hello as, once welcomed.
    welcomed: Mutex<Option<String>>,
    /// The topics it follows already, each followed once for the session.
    followed: Mutex<Vec<Topic>>,
    sink: NoticeSink,
}

impl Session {
    /// Says hello as `client` unless it did already.
    fn welcome(&self, client: &ClientInfo) -> Result<(), ControlError> {
        let mut welcomed = self
            .welcomed
            .lock()
            .map_err(|poisoned| ControlError::internal(poisoned.to_string()))?;
        if welcomed.as_deref() == Some(client.name.as_str()) {
            return Ok(());
        }
        self.client.hello(client.clone())?;
        *welcomed = Some(client.name.clone());
        Ok(())
    }
}

impl Backend for Session {
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<&str>,
        command: Command,
    ) -> Result<Value, ControlError> {
        self.welcome(client)?;
        self.client.call(request_id.map(str::to_owned), command)
    }

    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError> {
        self.welcome(client)?;
        let mut followed = self
            .followed
            .lock()
            .map_err(|poisoned| ControlError::internal(poisoned.to_string()))?;
        let new: Vec<Topic> = (topics.iter())
            .filter(|topic| !followed.contains(topic))
            .copied()
            .collect();
        if new.is_empty() {
            return Ok(());
        }
        let sink = Arc::clone(&self.sink);
        (self.client).subscribe(new.clone(), move |topic| sink(Notice::Changed(topic)))?;
        followed.extend(new);
        Ok(())
    }

    fn unsubscribe(&self, _topics: &[Topic]) -> Result<(), ControlError> {
        // The server ignores the topics it no longer watches; they're let go with the session.
        Ok(())
    }

    fn resubscribe(&self) -> Result<(), ControlError> {
        // An in-process session isn't lost while the app runs.
        Ok(())
    }
}
