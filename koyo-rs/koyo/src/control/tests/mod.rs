//! App control, driven as its clients drive it, headless: requests handed to the pipeline on
//! the UI thread as the socket's connections hand them, the person's answers simulated, and
//! the window's shell looked at after. Timers are the test clock's, so a two-minute prompt
//! times out when the test says.

mod activity;
mod approval;
mod chat_modes;
mod chat_set;
mod chats;
mod host;
mod panes;
mod pipeline;
mod projects;
mod settings;
mod shown;
mod socket;
mod tabs;
mod viewport;
mod worktrees;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::{AnyWindowHandle, App, Entity, TestAppContext};
use koyo_app_control::{ClientInfo, Command, ControlError, Hello, PROTOCOL_VERSION, Scope};
use serde_json::Value;

use super::{Answer, ApprovedClient, Caller, Control, Request, Timeouts, Welcome, handle};
use crate::settings::BranchPrefix;
use crate::shell::Shell;
use crate::test_support::{scratch, shell_with};

/// How long the prompt allowing a client waits in these tests.
const APPROVAL: Duration = Duration::from_secs(5);
/// How long a denial is remembered in these tests.
const DENIAL: Duration = Duration::from_secs(20);
/// How long the prompt confirming a destructive call waits in these tests.
const CONFIRM: Duration = Duration::from_secs(5);

/// A window under app control: the window, its shell, the channel's reading end the socket's
/// connections and in-process clients hand their requests to, and the folder new worktrees
/// go in.
struct Rig {
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    requests: UnboundedReceiver<Request>,
    worktrees: PathBuf,
}

/// The shell over `roots` under app control, without its socket: the prompts' times
/// shortened, new worktrees named without a prefix in a scratch folder of `name`'s.
fn controlled(name: &str, roots: &[&Path], cx: &mut TestAppContext) -> Rig {
    // First, so app control follows the shell made next.
    let requests = cx.update(super::start_for_test);
    let (window, shell) = shell_with(roots, cx);
    let worktrees = scratch(&format!("{name}-worktrees"));
    cx.update(|cx| {
        Control::get_mut(cx).timeouts = Timeouts {
            approval: APPROVAL,
            denial_memory: DENIAL,
            confirm: CONFIRM,
        };
        let folder = worktrees.clone();
        crate::settings::update(cx, |settings| {
            settings.worktrees_dir = Some(folder);
            settings.branch_prefix = BranchPrefix::None;
        });
    });
    cx.run_until_parked();
    Rig {
        window,
        shell,
        requests,
        worktrees,
    }
}

/// An agent in Koyo terminal `handle`.
fn terminal(handle: &str) -> Caller {
    Caller {
        terminal: Some(handle.into()),
        ..external("koyo app")
    }
}

/// The agent of Koyo chat `id`, as Koyo's MCP server in its session says hello.
fn chat(id: &str) -> Caller {
    Caller {
        chat: Some(id.into()),
        ..external("claude-code")
    }
}

/// An external client called `name`.
fn external(name: &str) -> Caller {
    Caller {
        client: ClientInfo {
            name: name.into(),
            version: "test".into(),
        },
        terminal: None,
        chat: None,
        worktree: None,
        cwd: None,
    }
}

/// The hello `caller` says.
fn hello_of(caller: &Caller) -> Hello {
    Hello {
        protocol: PROTOCOL_VERSION,
        client: caller.client.clone(),
        terminal: caller.terminal.clone(),
        chat: caller.chat.clone(),
        worktree: None,
        cwd: None,
        pid: 1,
    }
}

/// Where an answer lands once it's given.
type Slot<T> = Arc<Mutex<Option<Result<T, ControlError>>>>;

fn slot<T: Send + 'static>() -> (Slot<T>, Answer<T>) {
    let slot: Slot<T> = Arc::new(Mutex::new(None));
    let landing = Arc::clone(&slot);
    (
        slot,
        Box::new(move |result| *landing.lock().unwrap() = Some(result)),
    )
}

/// The answer in `slot`, once given.
fn taken<T>(slot: &Slot<T>) -> Option<Result<T, ControlError>> {
    slot.lock().unwrap().take()
}

/// Says `caller`'s hello, answered or not yet.
fn send_hello(caller: &Caller, cx: &mut TestAppContext) -> Slot<Welcome> {
    let (answered, reply) = slot();
    let hello = hello_of(caller);
    cx.update(|cx| handle(Request::Hello { hello, reply }, cx));
    answered
}

/// Says `caller`'s hello and waits for what's pending: its answer.
fn hello(caller: &Caller, cx: &mut TestAppContext) -> Result<Welcome, ControlError> {
    let answered = send_hello(caller, cx);
    cx.run_until_parked();
    taken(&answered).expect("the hello is answered")
}

/// Makes `caller`'s call of `command`, answered or not yet.
fn send(
    caller: &Caller,
    request_id: Option<&str>,
    command: Command,
    cx: &mut TestAppContext,
) -> Slot<Value> {
    let (answered, reply) = slot();
    let request = Request::Call(super::pipeline::Pending {
        caller: caller.clone(),
        request_id: request_id.map(Into::into),
        command,
        reply,
    });
    cx.update(|cx| handle(request, cx));
    answered
}

/// Makes `caller`'s call of `operation` with `input`, and waits for what's pending: its
/// answer.
fn call(
    caller: &Caller,
    operation: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> Result<Value, ControlError> {
    let answered = send(caller, None, op(operation, input), cx);
    cx.run_until_parked();
    taken(&answered).expect("the call is answered")
}

/// The call of `operation` with `input`.
fn op(operation: &str, input: Value) -> Command {
    Command::parse(operation, input).unwrap()
}

/// Turns external control on with client `name` allowed `scopes`, as the person would in
/// Settings.
fn allow(name: &str, scopes: &[Scope], cx: &mut TestAppContext) {
    let client = ApprovedClient {
        name: name.into(),
        scopes: scopes.to_vec(),
        approved_at: 0,
        last_used: None,
    };
    cx.update(|cx| {
        crate::settings::update(cx, |settings| {
            settings.external_control.enabled = true;
            settings.external_control.clients.push(client);
        });
    });
}

/// The activity log, newest first.
fn activity(cx: &TestAppContext) -> Vec<super::ActivityEntry> {
    cx.read(|cx| super::activity(cx).cloned().collect())
}

/// Reads `read` off the shell.
fn read_shell<R>(rig: &Rig, cx: &TestAppContext, read: impl FnOnce(&Shell, &App) -> R) -> R {
    cx.read(|cx| read(rig.shell.read(cx), cx))
}

/// The path of the worktree a call's output names.
fn path_of(value: &Value) -> PathBuf {
    PathBuf::from(value.as_str().expect("a path"))
}
