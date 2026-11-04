//! Chats, as a person's go: through the host, which runs them whether a tab shows them or not.

use std::path::PathBuf;

use gpui_kit::{App, AppContext as _, AsyncApp, Entity};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::methods::{Delivery, Rows};
use koyo_agent::protocol::model::{ItemBody, Prompt, Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;
use koyo_app_control::ControlError;
use koyo_app_control::ops::chat::{
    ChatCreateInput, ChatCreated, ChatInfo, ChatInput, ChatListInput, ChatListOutput, ChatMessage,
    ChatNeeds, ChatReadInput, ChatReadOutput, ChatRole, ChatSendInput, ChatSent, ChatStatus,
};
use serde_json::Value;

use super::super::Caller;
use super::{Exec, info, main_shell, output, select, try_update_shell, window_closed};
use crate::chat::Chats;
use crate::shell::Shell;

/// How many turns of a chat are read when the input doesn't say.
const TURNS_READ: u32 = 5;
/// The most turns of a chat a read gets.
const TURNS_MOST: u32 = 30;

/// The chats of the worktree `input` names, or of every worktree.
pub(crate) fn list(
    caller: &Caller,
    input: &ChatListInput,
    cx: &App,
) -> Result<Value, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let within = if input.all == Some(true) {
        None
    } else {
        Some(worktree_path(caller, shell, input.worktree.as_deref())?)
    };
    let chats = shell.chats().read(cx).latest_first(within.as_deref());
    output(&ChatListOutput { chats: chats.into_iter().map(chat_info).collect() })
}

/// The checkout of the worktree `selector` names.
fn worktree_path(
    caller: &Caller,
    shell: &Shell,
    selector: Option<&str>,
) -> Result<PathBuf, ControlError> {
    let (project, branch) = select::worktree(caller, &shell.workspace, selector, None)?;
    Ok(shell.workspace.projects[project].branches[branch].worktree.path.clone())
}

fn chat_info(summary: &ThreadSummary) -> ChatInfo {
    ChatInfo {
        id: summary.id.0.clone(),
        title: summary.title.clone(),
        worktree: summary.cwd.clone(),
        status: status(summary.status),
        needs: summary.needs.map(|needs| match needs {
            RequestKind::Approval => ChatNeeds::Approval,
            RequestKind::Question => ChatNeeds::Question,
            RequestKind::Plan => ChatNeeds::Plan,
        }),
        queued: summary.queued,
        unread: summary.unread,
        open: summary.tab_open,
        updated_at: summary.updated_at.0.clone(),
    }
}

fn status(status: Status) -> ChatStatus {
    match status {
        Status::Idle => ChatStatus::Idle,
        Status::Working => ChatStatus::Working,
        Status::Failed => ChatStatus::Failed,
    }
}

/// What was said in the latest turns of the chat `input` names.
pub(crate) fn read(input: &ChatReadInput, cx: &mut App) -> Result<Exec, ControlError> {
    let id = ThreadId(input.chat.clone());
    let (chats, doing) = known(&id, cx)?;
    let turns = input.turns.unwrap_or(TURNS_READ).clamp(1, TURNS_MOST);
    let rows = chats.read(cx).history(&id, None, turns, cx);
    Ok(Exec::Later(cx.background_spawn(async move {
        let rows = rows.await.map_err(|error| refused("read the chat", &error))?;
        output(&ChatReadOutput { status: status(doing), messages: messages(rows) })
    })))
}

fn messages(rows: Rows) -> Vec<ChatMessage> {
    (rows.items.into_iter())
        .filter_map(|item| match item.body {
            ItemBody::UserMessage(message) => {
                Some(ChatMessage { role: ChatRole::User, text: message.text })
            }
            ItemBody::AssistantText(text) => {
                Some(ChatMessage { role: ChatRole::Assistant, text: text.text })
            }
            _ => None,
        })
        .collect()
}

/// Shows the chat `input` names.
pub(crate) fn open(input: &ChatInput, cx: &mut App) -> Result<Value, ControlError> {
    let id = ThreadId(input.chat.clone());
    try_update_shell(cx, |shell, _, cx| {
        let pane = shell.open_chat(&id, cx).ok_or_else(|| {
            ControlError::not_found(format!(
                "No chat in a checkout Koyo lists has the id `{id}`. `chat_list` lists them."
            ))
        })?;
        output(&info::place(shell, pane)?)
    })
}

/// Starts a chat in the worktree `input` names, in a tab of its own, and says its text to it.
pub(crate) fn create(
    caller: &Caller,
    input: ChatCreateInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let prompt = prompt(input.text)?;
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let cwd = worktree_path(caller, shell, input.worktree.as_deref())?;
    let chats = shell.chats().clone();
    let made = chats.read(cx).create(&cwd);
    let select = input.select != Some(false);
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let thread = made.await.map_err(|error| refused("start a chat", &error))?;
        let id = thread.id;
        let sent = cx.update(|cx| chats.read(cx).send(&id, prompt, Delivery::Auto));
        sent.await.map_err(|error| refused("say that to the new chat", &error))?;
        cx.update(|cx| {
            try_update_shell(cx, |shell, _, cx| {
                let pane = shell.add_chat(&cwd, id.clone(), select, cx).ok_or_else(|| {
                    ControlError::not_found("The worktree went before its chat could show.")
                })?;
                output(&ChatCreated { chat: id.0.clone(), place: info::place(shell, pane)? })
            })
        })
    })))
}

/// Says the text of `input` to its chat.
pub(crate) fn send(input: ChatSendInput, cx: &mut App) -> Result<Exec, ControlError> {
    let id = ThreadId(input.chat);
    let prompt = prompt(input.text)?;
    let delivery = if input.queue == Some(true) { Delivery::Queue } else { Delivery::Auto };
    let sent = known(&id, cx)?.0.read(cx).send(&id, prompt, delivery);
    Ok(Exec::Later(cx.spawn(async move |_: &mut AsyncApp| {
        let sent = sent.await.map_err(|error| refused("say that to the chat", &error))?;
        output(&ChatSent { turn: sent.run_id.0 })
    })))
}

/// Asks what the chat `input` names is running to stop.
pub(crate) fn stop(input: &ChatInput, cx: &mut App) -> Result<Value, ControlError> {
    let id = ThreadId(input.chat.clone());
    known(&id, cx)?.0.update(cx, |chats, cx| chats.stop(&id, cx));
    output(input)
}

/// The chats and how `id` is doing, if it's one of them. A subagent's isn't: it's its parent's to
/// run.
fn known(id: &ThreadId, cx: &App) -> Result<(Entity<Chats>, Status), ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let chats = shell.read(cx).chats().clone();
    let doing = chats.read(cx).summary(id).map(|summary| summary.status);
    let doing = doing.ok_or_else(|| {
        ControlError::not_found(format!("No chat has the id `{id}`. `chat_list` lists them."))
    })?;
    Ok((chats, doing))
}

fn prompt(text: String) -> Result<Prompt, ControlError> {
    if text.trim().is_empty() {
        return Err(ControlError::invalid_input("There's nothing in `text` to say."));
    }
    Ok(Prompt { message_id: MessageId::fresh(), text, attachments: Vec::new() })
}

fn refused(what: &str, error: &ClientError) -> ControlError {
    ControlError::internal(format!("Koyo couldn't {what}: {error}"))
}
