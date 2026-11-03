//! App control (`docs/app-control.md`): the app's side of the control socket. A listener
//! serves the data folder's socket while the app runs, a thread per connection, and hands
//! each hello, call and subscription to the UI thread on one channel. There the pipeline
//! checks the caller and its grant, replays or records the call's receipt, asks the person to
//! confirm a destructive call, runs the executor on the window's shell, logs the call and
//! answers. A caller that isn't on the socket, the MCP server over HTTP, hands the same
//! requests in through [`InProcess`]. The state lives in a global rather than in the shell,
//! as the window may close and open again while the app runs.

mod activity;
mod auth;
mod caller;
mod confirm;
mod exec;
mod gate;
mod http;
mod in_process;
mod listener;
mod pipeline;
mod policy;
mod receipts;
mod topics;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use koyo_app_control::transport::{BindError, Listener};
use koyo_app_control::{CONFIRM_TIMEOUT, ControlError, Endpoint, Hello, Scope, Topic};
use koyo_term::TermClient;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, Global, Subscription};

// What the Settings page reads and changes.
pub use activity::{ActivityEntry, ActivityKind, Decision};
pub(crate) use caller::Caller;
pub use http::state as http_state;
#[cfg(test)]
pub(crate) use in_process::InProcess;
#[cfg(test)]
pub(crate) use policy::{ApprovedClient, set_enabled};
pub use policy::{
    DEFAULT_PORT, Policy, activity, allow_denied, denied, policy, revoke, set_client_scopes,
    set_port,
};

use crate::settings;
use crate::shell::Shell;

/// How long the person's prompts wait, and how long a denial is remembered; tests shorten
/// them.
pub(crate) struct Timeouts {
    /// How long the prompt allowing a client waits for an answer.
    pub approval: Duration,
    /// How long a client the person denied is refused without asking again.
    pub denial_memory: Duration,
    /// How long the prompt confirming a destructive call waits for an answer.
    pub confirm: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            approval: auth::APPROVAL_TIMEOUT,
            denial_memory: auth::DENIAL_MEMORY,
            confirm: CONFIRM_TIMEOUT,
        }
    }
}

/// How a request is answered: called once, on the UI thread, with the outcome.
pub(crate) type Answer<T> = Box<dyn FnOnce(Result<T, ControlError>) + Send>;

/// What the UI thread is asked by a connection.
pub(crate) enum Request {
    /// A client said hello: welcome it with its grant, ask the person, or refuse it.
    Hello {
        hello: Hello,
        reply: Answer<Welcome>,
    },
    /// A welcomed client called an operation.
    Call(pipeline::Pending),
    /// A welcomed client follows topics as well, each change told to `sink`.
    Subscribe {
        connection: u64,
        caller: Caller,
        topics: Vec<Topic>,
        sink: topics::Sink,
        reply: Answer<()>,
    },
    /// A connection that followed topics ended.
    Closed { connection: u64 },
}

/// A hello accepted: who calls, and what it may do.
#[derive(Clone, Debug)]
pub(crate) struct Welcome {
    pub caller: Caller,
    pub grant: Vec<Scope>,
}

/// What app control keeps while the app runs.
pub(crate) struct Control {
    /// Where connections hand their requests to the UI thread.
    requests: UnboundedSender<Request>,
    /// The control socket's listener, while it serves.
    server: Option<listener::Server>,
    pub(crate) timeouts: Timeouts,
    /// Numbers the prompts and the denials, so a late answer finds whether it's still due.
    next: u64,
    /// Hellos waiting on the prompt allowing their client, by client name.
    approvals: HashMap<String, auth::Approval>,
    /// Clients the person denied lately, with the denial's number.
    denied: HashMap<String, u64>,
    /// Calls waiting on the person's confirmation, one per client.
    confirmations: HashMap<String, confirm::Waiting>,
    receipts: receipts::Receipts,
    activity: activity::Activity,
    hub: topics::Hub,
    /// The MCP server over HTTP, while the policy has it on and its port could be bound.
    http: Option<http::Serving>,
    /// Why the policy's port couldn't be served.
    http_error: Option<String>,
    /// The terminal daemon's client, to tell an agent's terminal is real; made on first use.
    terminal_client: Option<Arc<TermClient>>,
    /// Terminals tests say exist, as no daemon runs in them.
    #[cfg(test)]
    pub(crate) test_terminals: std::collections::HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl Global for Control {}

impl Control {
    fn get(cx: &App) -> &Control {
        cx.global::<Control>()
    }

    fn get_mut(cx: &mut App) -> &mut Control {
        cx.global_mut::<Control>()
    }

    /// The next number for a prompt or a denial.
    fn number(cx: &mut App) -> u64 {
        let control = Control::get_mut(cx);
        control.next += 1;
        control.next
    }
}

/// Numbers connections, the socket's and the in-process ones alike, so the topics they follow
/// are told apart.
fn next_connection() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Starts app control as the app starts: serves the data folder's control socket whatever
/// the policy says, as the agents in Koyo's terminals call through it either way, and
/// answers what comes in on the UI thread. Another Koyo answering on the same socket
/// leaves this one without app control.
pub fn start(cx: &mut App) {
    let received = install(cx);
    let endpoint = Endpoint::from_env();
    let bound = endpoint
        .as_ref()
        .map_err(Clone::clone)
        .map(|endpoint| (endpoint.clone(), Listener::bind(endpoint)));
    match bound {
        Ok((endpoint, Ok(listener))) => {
            let requests = Control::get(cx).requests.clone();
            match listener::Server::start(listener, endpoint, requests) {
                Ok(server) => {
                    log::info!("app control listens on {}", server.path().display());
                    Control::get_mut(cx).server = Some(server);
                }
                Err(error) => log::error!("couldn't serve app control: {error}"),
            }
        }
        Ok((_, Err(error @ BindError::InUse(_)))) => log::warn!("{error}"),
        Ok((_, Err(error))) => log::error!("{error}"),
        Err(error) => log::error!("couldn't find the control socket: {error}"),
    }
    // The MCP server over HTTP follows the policy, which lives in the settings.
    http::apply(cx);
    let following = settings::observe_app(cx, http::apply);
    Control::get_mut(cx)._subscriptions.push(following);
    cx.spawn(async move |cx| {
        let mut received = received;
        while let Some(request) = received.next().await {
            cx.update(|cx| handle(request, cx));
        }
    })
    .detach();
    // The socket goes with the app, so the next one binds it without a stale file.
    cx.on_app_quit(|cx| {
        let control = Control::get_mut(cx);
        control.server.take();
        control.http.take();
        async {}
    })
    .detach();
}

/// Sets up app control without its socket or its channel's reader: tests hand requests to
/// [`handle`] themselves, as the test scheduler takes no wake-ups from other threads.
#[cfg(test)]
pub(crate) fn start_for_test(cx: &mut App) -> UnboundedReceiver<Request> {
    install(cx)
}

/// Puts app control's state in place, following the shell's and the settings' changes for
/// the topics; the channel's reading end.
fn install(cx: &mut App) -> UnboundedReceiver<Request> {
    let (requests, received) = unbounded();
    let shells = cx.observe_new::<Shell>(|_, _, cx| {
        let observer = cx.observe_self(|_, cx| topics::mark(&[Topic::App, Topic::Workspace], cx));
        if cx.has_global::<Control>() {
            Control::get_mut(cx).hub.shell_observer = Some(observer);
        }
    });
    let settings = topics::follow_settings(cx);
    cx.set_global(Control {
        requests,
        server: None,
        timeouts: Timeouts::default(),
        next: 0,
        approvals: HashMap::new(),
        denied: HashMap::new(),
        confirmations: HashMap::new(),
        receipts: receipts::Receipts::default(),
        activity: activity::Activity::default(),
        hub: topics::Hub::default(),
        http: None,
        http_error: None,
        terminal_client: None,
        #[cfg(test)]
        test_terminals: std::collections::HashSet::new(),
        _subscriptions: vec![shells, settings],
    });
    received
}

/// Hands the UI thread the request `make` makes and waits up to `timeout` for its answer.
fn ask<T: Send + 'static>(
    requests: &UnboundedSender<Request>,
    timeout: Duration,
    make: impl FnOnce(Answer<T>) -> Request,
) -> Result<T, ControlError> {
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    let reply: Answer<T> = Box::new(move |result| {
        answer.send(result).ok();
    });
    requests
        .unbounded_send(make(reply))
        .map_err(|_closed| ControlError::app_not_running())?;
    match answered.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(ControlError::timeout(
            "Koyo didn't answer in time; the call may or may not have taken effect.",
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(ControlError::app_not_running())
        }
    }
}

/// Answers one request on the UI thread.
pub(crate) fn handle(request: Request, cx: &mut App) {
    match request {
        Request::Hello { hello, reply } => auth::hello(&hello, reply, cx),
        Request::Call(pending) => pipeline::call(pending, cx),
        Request::Subscribe {
            connection,
            caller,
            topics,
            sink,
            reply,
        } => topics::subscribe(connection, &caller, &topics, sink, reply, cx),
        Request::Closed { connection } => topics::closed(connection, cx),
    }
}

/// A setting changed, the control policy maybe: with external control off, the hellos of
/// external clients waiting on the person are refused, and followers who may no longer read
/// stop hearing. A destructive call waiting on the person is checked against the policy
/// again once they've answered.
fn policy_changed(cx: &mut App) {
    if !settings::get(cx).external_control.enabled {
        auth::refuse_all(&ControlError::control_disabled(), cx);
    }
    topics::drop_refused(cx);
}
