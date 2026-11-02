//! App control for a caller inside the app's process, the MCP server over HTTP: the same
//! hello, gate, prompts, receipts and log as the socket's clients get, through the same
//! channel to the UI thread. Its calls block, so they're made from threads of their own,
//! never from the UI thread, which answers them.

use std::sync::Mutex;
use std::sync::mpsc;

use colosso_app_control::{
    CALL_TIMEOUT, CONFIRM_TIMEOUT, ClientInfo, Command, ControlError, HELLO_TIMEOUT, Hello, Scope,
    Topic, check_request_id,
};
use futures::channel::mpsc::UnboundedSender;
#[cfg(test)]
use gpui_kit::App;
use serde_json::Value;

#[cfg(test)]
use super::Control;
use super::pipeline::Pending;
use super::{Caller, Request, ask, next_connection};

/// One in-process client: say hello first, then call and follow topics.
pub struct InProcess {
    requests: UnboundedSender<Request>,
    /// Who it is once welcomed.
    caller: Mutex<Option<Caller>>,
    /// The connections its subscriptions follow topics on, let go when it's dropped.
    connections: Mutex<Vec<u64>>,
}

impl InProcess {
    /// A client of the running app's control; `None` until app control has started.
    #[cfg(test)]
    pub fn new(cx: &App) -> Option<InProcess> {
        let control = cx.try_global::<Control>()?;
        Some(InProcess::from_requests(control.requests.clone()))
    }

    /// A client handing its requests to app control on `requests`, made on any thread.
    pub(crate) fn from_requests(requests: UnboundedSender<Request>) -> InProcess {
        InProcess {
            requests,
            caller: Mutex::new(None),
            connections: Mutex::new(Vec::new()),
        }
    }

    /// Says hello as `client`, an external client: answers its grant once welcomed, which
    /// may wait on the person allowing it for up to two minutes.
    ///
    /// # Errors
    /// `control_disabled` while external control is off, `user_declined` when the person
    /// denies it or doesn't answer, `invalid_input` for a name that isn't allowed, and
    /// `app_not_running` when the app is quitting.
    pub fn hello(&self, client: ClientInfo) -> Result<Vec<Scope>, ControlError> {
        let hello = Hello::new(client);
        let welcome = ask(&self.requests, HELLO_TIMEOUT, |reply| Request::Hello {
            hello,
            reply,
        })?;
        if let Ok(mut caller) = self.caller.lock() {
            *caller = Some(welcome.caller);
        }
        Ok(welcome.grant)
    }

    /// Calls `command`, with `request_id` making a repeat of a changing call answer the first
    /// result instead of acting again: its output, once the app has carried it out.
    ///
    /// # Errors
    /// Whatever the call fails with; `invalid_input` before a hello or for a malformed
    /// `request_id`; `timeout` when the app takes longer than a call may.
    pub fn call(
        &self,
        request_id: Option<String>,
        command: Command,
    ) -> Result<Value, ControlError> {
        check_request_id(request_id.as_deref())?;
        let caller = self.caller()?;
        // A call the person may have to confirm waits for them as well.
        let timeout = if command.confirmed() {
            CALL_TIMEOUT + CONFIRM_TIMEOUT
        } else {
            CALL_TIMEOUT
        };
        ask(&self.requests, timeout, |reply| {
            Request::Call(Pending {
                caller,
                request_id,
                command,
                reply,
            })
        })
    }

    /// Follows `topics`, `sink` told each change on a thread of its own until this client is
    /// dropped.
    ///
    /// # Errors
    /// `scope_denied` without the `read` scope; `invalid_input` before a hello.
    pub fn subscribe(
        &self,
        topics: Vec<Topic>,
        sink: impl Fn(Topic) + Send + 'static,
    ) -> Result<(), ControlError> {
        let caller = self.caller()?;
        let connection = next_connection();
        let (tell, told) = mpsc::channel::<Topic>();
        std::thread::Builder::new()
            .name("app-control-events".into())
            .spawn(move || {
                for topic in told {
                    sink(topic);
                }
            })
            .map_err(|error| {
                ControlError::internal(format!("Colosso couldn't follow topics: {error}"))
            })?;
        let sink: super::topics::Sink = Box::new(move |topic| tell.send(topic).is_ok());
        ask(&self.requests, CALL_TIMEOUT, |reply| Request::Subscribe {
            connection,
            caller,
            topics,
            sink,
            reply,
        })?;
        if let Ok(mut connections) = self.connections.lock() {
            connections.push(connection);
        }
        Ok(())
    }

    fn caller(&self) -> Result<Caller, ControlError> {
        (self.caller.lock().ok())
            .and_then(|caller| caller.clone())
            .ok_or_else(|| ControlError::invalid_input("Say hello before calling."))
    }
}

impl Drop for InProcess {
    fn drop(&mut self) {
        let connections = self
            .connections
            .lock()
            .map(|connections| connections.clone())
            .unwrap_or_default();
        for connection in connections {
            self.requests
                .unbounded_send(Request::Closed { connection })
                .ok();
        }
    }
}
