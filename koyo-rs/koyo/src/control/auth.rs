//! Hellos (`docs/app-control.md` 4). An agent in a Koyo terminal is welcomed with what
//! such agents may do once the terminal daemon says its terminal exists, and a chat's agent
//! once the window knows the chat, while Koyo tools are on. Any other client is
//! external: refused while external control is off; welcomed with its scopes once the person
//! has allowed it; else the person is asked in the window, once per client name however many
//! hellos wait, for two minutes. A Deny is remembered for ten minutes.

use std::time::Duration;

use gpui_kit::{App, AppContext as _, AsyncApp, PromptLevel};
use koyo_agent::protocol::ids::ThreadId;
use koyo_app_control::{ControlError, ErrorCode, Hello, PROTOCOL_VERSION, Scope};
use koyo_term_control::{Handle, RequestBody, ResponseBody};

use super::activity::{ActivityKind, Decision};
use super::confirm::answered;
use super::exec::main_shell;
use super::{Answer, Caller, Control, Welcome, gate, policy};
use crate::chat::Connection;
use crate::settings;

/// How long the prompt allowing a client waits for an answer.
pub(crate) const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a client the person denied is refused without asking again.
pub(crate) const DENIAL_MEMORY: Duration = Duration::from_secs(600);

/// The hellos of one client name waiting on the person, and the prompt's number.
pub(crate) struct Approval {
    number: u64,
    waiting: Vec<(Caller, Answer<Welcome>)>,
}

/// Answers a hello: a welcome with the grant, or why not.
pub(crate) fn hello(hello: &Hello, reply: Answer<Welcome>, cx: &mut App) {
    if hello.protocol != PROTOCOL_VERSION {
        return reply(Err(ControlError::invalid_input(format!(
            "This client speaks control protocol {}, Koyo {PROTOCOL_VERSION}; use the koyo executable of the running app.",
            hello.protocol
        ))));
    }
    let caller = Caller::from_hello(hello);
    match (caller.chat.clone(), caller.terminal.clone()) {
        (Some(id), _) => chat_hello(caller, &id, reply, cx),
        (None, Some(handle)) => terminal_hello(caller, handle, reply, cx),
        (None, None) => external_hello(caller, reply, cx),
    }
}

/// Welcomes the agent of chat `id` with what the gate grants it, once the window knows the chat.
/// Before the window has heard its chats from the host, a chat it doesn't know may yet be one,
/// so it may say hello again.
fn chat_hello(caller: Caller, id: &str, reply: Answer<Welcome>, cx: &mut App) {
    let grant = match gate::grant(&caller, cx) {
        Ok(grant) => grant,
        Err(refused) => return reply(Err(refused)),
    };
    let (known, heard) = main_shell(cx).map_or((false, false), |(_, shell)| {
        let chats = shell.read(cx).chats().read(cx);
        let known = chats.summary(&ThreadId(id.to_owned())).is_some();
        (known, *chats.connection() == Connection::Connected)
    });
    reply(match (known, heard) {
        (true, _) => Ok(Welcome { caller, grant }),
        (false, true) => Err(ControlError::not_found(format!("Koyo has no chat {id}."))),
        (false, false) => Err(ControlError::not_found(
            "Koyo's window hasn't heard its chats from the agent host yet; try again in a moment.",
        )
        .retryable(true)),
    });
}

/// Welcomes an agent in Koyo terminal `handle`, once the daemon says the terminal exists.
fn terminal_hello(caller: Caller, handle: String, reply: Answer<Welcome>, cx: &mut App) {
    let welcome = |caller| Welcome {
        caller,
        grant: Scope::AGENT.to_vec(),
    };
    let unknown = move || {
        ControlError::new(
            ErrorCode::TerminalNotFound,
            format!(
                "Koyo runs no terminal {handle}. Unset KOYO_TERMINAL to call as an external client."
            ),
        )
    };
    if known_here(caller.terminal.as_deref().unwrap_or_default(), cx) {
        return reply(Ok(welcome(caller)));
    }
    let Some(client) = daemon(cx) else {
        return reply(Err(unknown()));
    };
    let wanted = Handle(caller.terminal.clone().unwrap_or_default());
    let listing = cx.background_spawn(async move {
        client.request_if_running(RequestBody::ListTerminals {
            worktree: None,
            status: None,
        })
    });
    cx.spawn(async move |_: &mut AsyncApp| {
        let known = match listing.await {
            Ok(Some(ResponseBody::Terminals { terminals })) => terminals
                .iter()
                .any(|summary| summary.record.handle == wanted),
            Ok(_) => false,
            Err(error) => {
                log::warn!("couldn't ask the terminal daemon about {wanted}: {error}");
                false
            }
        };
        reply(if known {
            Ok(welcome(caller))
        } else {
            Err(unknown())
        });
    })
    .detach();
}

/// Whether the window knows terminal `handle` already, or a test says it exists.
fn known_here(handle: &str, cx: &App) -> bool {
    #[cfg(test)]
    if Control::get(cx).test_terminals.contains(handle) {
        return true;
    }
    main_shell(cx)
        .is_some_and(|(_, shell)| shell.read(cx).terminal_known(&Handle(handle.to_owned())))
}

/// The terminal daemon's client, made on first use.
fn daemon(cx: &mut App) -> Option<std::sync::Arc<koyo_term::TermClient>> {
    let control = Control::get_mut(cx);
    if control.terminal_client.is_none() {
        control.terminal_client = crate::terminals::daemon_client();
    }
    control.terminal_client.clone()
}

/// Answers an external client's hello.
fn external_hello(caller: Caller, reply: Answer<Welcome>, cx: &mut App) {
    let name = caller.client.name.clone();
    if name.trim().is_empty() || name.starts_with("terminal:") || name.starts_with("chat:") {
        return reply(Err(ControlError::invalid_input(
            "A client's name can't be empty or start with `terminal:` or `chat:`.",
        )));
    }
    let policy = &settings::get(cx).external_control;
    if !policy.enabled {
        return reply(Err(ControlError::control_disabled()));
    }
    if let Some(client) = policy.client(&name) {
        let grant = client.scopes.clone();
        policy::touch(&name, cx);
        return reply(Ok(Welcome { caller, grant }));
    }
    let control = Control::get_mut(cx);
    if control.denied.contains_key(&name) {
        return reply(Err(ControlError::user_declined(format!(
            "The person denied “{name}” control of Koyo lately. They can allow it in Koyo under Settings → MCP server."
        ))));
    }
    if let Some(approval) = control.approvals.get_mut(&name) {
        approval.waiting.push((caller, reply));
        return;
    }
    ask(caller, reply, cx);
}

/// Asks the person whether `caller`'s client may control Koyo, opening the window to ask
/// in when it's closed.
fn ask(caller: Caller, reply: Answer<Welcome>, cx: &mut App) {
    let name = caller.client.name.clone();
    if main_shell(cx).is_none()
        && let Err(error) = crate::open_main_window(cx)
    {
        log::error!("couldn't open the window to ask about {name}: {error:#}");
    }
    let Some((window, _)) = main_shell(cx) else {
        return reply(Err(ControlError::internal(
            "Koyo couldn't open its window to ask the person.",
        )));
    };
    let question = format!("Allow “{name}” to control Koyo?");
    let detail = "It could read what the window shows and change it: select projects and worktrees, open and close tabs, add projects, create worktrees and save files. Settings → MCP server lists what each client may do.";
    let prompted = window.update(cx, |_, window, cx| {
        window.activate_window();
        window.prompt(
            PromptLevel::Info,
            &question,
            Some(detail),
            &["Allow", "Deny"],
            cx,
        )
    });
    let answer = match prompted {
        Ok(answer) => answer,
        Err(error) => {
            return reply(Err(ControlError::internal(format!(
                "Koyo couldn't ask the person: {error}"
            ))));
        }
    };
    let number = Control::number(cx);
    let control = Control::get_mut(cx);
    let waiting = vec![(caller, reply)];
    control
        .approvals
        .insert(name.clone(), Approval { number, waiting });
    let timeout = control.timeouts.approval;
    cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = answered(answer, timeout, cx).await;
        cx.update(|cx| decide(&name, number, decision, cx));
    })
    .detach();
}

/// Answers the hellos waiting on prompt `number` for client `name` as the person decided,
/// unless they were answered already.
fn decide(name: &str, number: u64, decision: Decision, cx: &mut App) {
    let control = Control::get_mut(cx);
    if control
        .approvals
        .get(name)
        .is_none_or(|approval| approval.number != number)
    {
        return;
    }
    let Some(approval) = control.approvals.remove(name) else {
        return;
    };
    control
        .activity
        .record(name.to_owned(), ActivityKind::Approval(decision));
    let refusal = match decision {
        Decision::Allowed => {
            policy::approve(name, cx);
            policy::touch(name, cx);
            let grant = (settings::get(cx).external_control.client(name)).map_or_else(
                || Scope::DEFAULT_GRANT.to_vec(),
                |client| client.scopes.clone(),
            );
            for (caller, reply) in approval.waiting {
                reply(Ok(Welcome {
                    caller,
                    grant: grant.clone(),
                }));
            }
            return;
        }
        Decision::Denied => {
            remember_denial(name, cx);
            ControlError::user_declined(format!("The person denied “{name}” control of Koyo."))
        }
        Decision::TimedOut => ControlError::user_declined(format!(
            "The person didn't answer whether “{name}” may control Koyo in time."
        )),
    };
    for (_, reply) in approval.waiting {
        reply(Err(refusal.clone()));
    }
}

/// Refuses `name` without asking until the denial passes.
fn remember_denial(name: &str, cx: &mut App) {
    let number = Control::number(cx);
    let control = Control::get_mut(cx);
    control.denied.insert(name.to_owned(), number);
    let memory = control.timeouts.denial_memory;
    let name = name.to_owned();
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(memory).await;
        cx.update(|cx| {
            let denied = &mut Control::get_mut(cx).denied;
            if denied.get(&name) == Some(&number) {
                denied.remove(&name);
            }
        });
    })
    .detach();
}

/// Refuses every hello waiting on the person with `error`.
pub(crate) fn refuse_all(error: &ControlError, cx: &mut App) {
    let approvals = std::mem::take(&mut Control::get_mut(cx).approvals);
    for (_, approval) in approvals {
        for (_, reply) in approval.waiting {
            reply(Err(error.clone()));
        }
    }
}
