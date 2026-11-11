//! Chats, as a person's go: through the host, which runs them whether a tab shows them or not.
//! What a chat runs with is told as its composer shows it, which is what runs (`choices`).

mod choices;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use futures::future::{OptionFuture, join_all};
use gpui_kit::{App, AppContext as _, AsyncApp, Entity};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::methods::{Catalog, Delivery, Rows, UpdateParams};
use koyo_agent::protocol::model::{Effort, ItemBody, Mode, Prompt, Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;
use koyo_app_control::ControlError;
use koyo_app_control::ops::chat::{
    ChatChanged, ChatChoicesInput, ChatCreateInput, ChatCreated, ChatInfo, ChatInput,
    ChatListInput, ChatListOutput, ChatMessage, ChatNeeds, ChatReadInput, ChatReadOutput, ChatRole,
    ChatSendInput, ChatSent, ChatSetInput, ChatStatus,
};
use serde_json::Value;

use self::choices::Asked;
use super::super::{Caller, policy};
use super::{Exec, info, main_shell, output, select, try_update_shell, window_closed};
use crate::chat::Chats;
use crate::chat::chosen::Settings;
use crate::navigation::Route;
use crate::settings;
use crate::shell::Shell;

/// How many turns of a chat are read when the input doesn't say.
const TURNS_READ: u32 = 5;
/// The most turns of a chat a read gets.
const TURNS_MOST: u32 = 30;

/// The chats of the worktree `input` names, or of every worktree, each with what it runs with
/// by its checkout's catalog, asked for once for each checkout. Without an answer, a chat is told
/// as far as it chose.
pub(crate) fn list(
    caller: &Caller,
    input: &ChatListInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let within = match input.all {
        Some(true) => None,
        _ => Some(worktree_path(
            caller,
            shell.read(cx),
            input.worktree.as_deref(),
        )?),
    };
    let chats = shell.read(cx).chats().clone();
    let summaries: Vec<ThreadSummary> = (chats.read(cx).latest_first(within.as_deref()))
        .into_iter()
        .cloned()
        .collect();
    let cwds: BTreeSet<String> = summaries
        .iter()
        .map(|summary| summary.cwd.clone())
        .collect();
    let asking: Vec<_> = (cwds.iter())
        .map(|cwd| chats.update(cx, |chats, cx| chats.ask_catalog(Path::new(cwd), cx)))
        .collect();
    let chat_effort = settings::get(cx).chat_effort;
    Ok(Exec::Later(cx.background_spawn(async move {
        let answers = join_all(asking).await.into_iter();
        let catalogs: BTreeMap<String, Catalog> = cwds
            .into_iter()
            .zip(answers.map(Result::unwrap_or_default))
            .collect();
        let chats = (summaries.iter())
            .map(|summary| chat_info(summary, &catalogs[&summary.cwd], chat_effort));
        output(&ChatListOutput {
            chats: chats.collect(),
        })
    })))
}

/// What the worktree `input` names offers its chats, and what a new one runs with.
pub(crate) fn choices(
    caller: &Caller,
    input: &ChatChoicesInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let cwd = worktree_path(caller, shell.read(cx), input.worktree.as_deref())?;
    let chats = shell.read(cx).chats().clone();
    let asking = chats.update(cx, |chats, cx| chats.ask_catalog(&cwd, cx));
    let chat_effort = settings::get(cx).chat_effort;
    Ok(Exec::Later(cx.background_spawn(async move {
        let catalog = asking.await.map_err(|error| refused(ASK_CHOICES, &error))?;
        output(&choices::choices(&catalog, chat_effort))
    })))
}

/// What's asked of the host to learn what a checkout's chats can run with.
const ASK_CHOICES: &str = "ask the agent what this worktree's chats can run with";

/// The checkout of the worktree `selector` names.
fn worktree_path(
    caller: &Caller,
    shell: &Shell,
    selector: Option<&str>,
) -> Result<PathBuf, ControlError> {
    let (project, branch) = select::worktree(caller, &shell.workspace, selector, None)?;
    Ok(shell.workspace.projects[project].branches[branch]
        .worktree
        .path
        .clone())
}

fn chat_info(summary: &ThreadSummary, catalog: &Catalog, chat_effort: Effort) -> ChatInfo {
    let chosen = Settings::of_summary(summary);
    ChatInfo {
        runs_with: choices::runs_with(&chosen, catalog, chat_effort),
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
    let (chats, summary) = known(&id, cx)?;
    let turns = input.turns.unwrap_or(TURNS_READ).clamp(1, TURNS_MOST);
    let rows = chats.read(cx).history(&id, None, turns, cx);
    Ok(Exec::Later(cx.background_spawn(async move {
        let rows = rows
            .await
            .map_err(|error| refused("read the chat", &error))?;
        output(&ChatReadOutput {
            status: status(summary.status),
            messages: messages(rows),
        })
    })))
}

fn messages(rows: Rows) -> Vec<ChatMessage> {
    (rows.items.into_iter())
        .filter_map(|item| match item.body {
            ItemBody::UserMessage(message) => Some(ChatMessage {
                role: ChatRole::User,
                text: message.text,
            }),
            ItemBody::AssistantText(text) => Some(ChatMessage {
                role: ChatRole::Assistant,
                text: text.text,
            }),
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

/// Starts a chat in the worktree `input` names, on what it asks for as the catalog of that
/// checkout offers it, in a tab of its own, and says its text to it: the tab shown when `select`
/// says, or as the person set what agents open to show.
pub(crate) fn create(
    caller: &Caller,
    input: ChatCreateInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let prompt = prompt(input.text.clone())?;
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let cwd = worktree_path(caller, shell, input.worktree.as_deref())?;
    let chats = shell.chats().clone();
    let asking = chats.update(cx, |chats, cx| chats.ask_catalog(&cwd, cx));
    let own = OptionFuture::from(caller_mode(caller, &chats, cx));
    let chat_effort = settings::get(cx).chat_effort;
    let select = policy::shows(input.select, cx);
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let catalog = asking.await.map_err(|error| refused(ASK_CHOICES, &error))?;
        let chosen = choices::chosen_for(&Asked::of(&input), &catalog, chat_effort, own.await)?;
        let made =
            cx.update(|cx| chats.update(cx, |chats, cx| chats.create(chosen.to_create(&cwd), cx)));
        let thread = made
            .await
            .map_err(|error| refused("start a chat", &error))?;
        let runs_with = choices::runs_with(&Settings::of(&thread), &catalog, chat_effort);
        let id = thread.id;
        let sent = cx.update(|cx| chats.read(cx).send(&id, prompt, Delivery::Auto));
        sent.await
            .map_err(|error| refused("say that to the new chat", &error))?;
        cx.update(|cx| {
            try_update_shell(cx, |shell, window, cx| {
                let pane = shell
                    .add_chat(&cwd, id.clone(), select, cx)
                    .ok_or_else(|| {
                        ControlError::not_found("The worktree went before its chat could show.")
                    })?;
                if select {
                    shell.navigate(Route::Workspace, cx);
                    shell.follow_selection(window, cx);
                }
                output(&ChatCreated {
                    chat: id.0.clone(),
                    place: info::place(shell, pane)?,
                    runs_with,
                })
            })
        })
    })))
}

/// Says the text of `input` to its chat, at the effort its composer shows, as the composer has a
/// chat take it with each message: one that chose none, or one its model can't run at, takes it
/// first, so what shows is what runs.
pub(crate) fn send(input: ChatSendInput, cx: &mut App) -> Result<Exec, ControlError> {
    let id = ThreadId(input.chat);
    let prompt = prompt(input.text)?;
    let delivery = if input.queue == Some(true) {
        Delivery::Queue
    } else {
        Delivery::Auto
    };
    let (chats, summary) = known(&id, cx)?;
    let asking = chats.update(cx, |chats, cx| {
        chats.ask_catalog(Path::new(&summary.cwd), cx)
    });
    let chat_effort = settings::get(cx).chat_effort;
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        // Without an answer, the chat's effort is left as it is.
        let chosen = Settings::of_summary(&summary);
        let catalog = asking.await.ok();
        let settled =
            catalog.and_then(|catalog| choices::to_settle(&chosen, &catalog, chat_effort));
        if let Some(effort) = settled {
            let params = UpdateParams {
                effort: Some(Some(effort)),
                ..UpdateParams::of(id.clone())
            };
            let changed = cx.update(|cx| chats.read(cx).change(params));
            changed
                .await
                .map_err(|error| refused("set the chat's effort", &error))?;
        }
        let sent = cx.update(|cx| chats.read(cx).send(&id, prompt, delivery));
        let sent = sent
            .await
            .map_err(|error| refused("say that to the chat", &error))?;
        output(&ChatSent {
            turn: sent.run_id.0,
        })
    })))
}

/// Changes what the chat `input` names runs with, as its composer's chips would, checked against
/// its checkout's catalog: the host hears only what changes, and the answer says what the chat
/// runs with then.
pub(crate) fn set(
    caller: &Caller,
    input: ChatSetInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let id = ThreadId(input.chat.clone());
    let (chats, summary) = known(&id, cx)?;
    if !Asked::of_set(&input).asks() {
        return Err(ControlError::invalid_input(
            "There's nothing to change: give a `model`, `effort`, `mode`, `fast` or `plan`.",
        ));
    }
    let asking = chats.update(cx, |chats, cx| {
        chats.ask_catalog(Path::new(&summary.cwd), cx)
    });
    let own = OptionFuture::from(caller_mode(caller, &chats, cx));
    let chat_effort = settings::get(cx).chat_effort;
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let catalog = asking.await.map_err(|error| refused(ASK_CHOICES, &error))?;
        let asked = Asked::of_set(&input);
        let current = Settings::of_summary(&summary);
        let next = choices::changed_by(&current, &asked, &catalog, own.await)?;
        let params = choices::changes(&current, &next, &asked, id);
        let changed = cx.update(|cx| chats.read(cx).change(params));
        let thread = changed
            .await
            .map_err(|error| refused("change the chat", &error))?;
        output(&ChatChanged {
            chat: input.chat,
            runs_with: choices::runs_with(&Settings::of(&thread), &catalog, chat_effort),
        })
    })))
}

/// The mode the chat whose agent calls runs in, above which it gives no chat a mode (decided
/// 2026-11-11): what it chose, or else what its checkout's chats get by the person's and the
/// agent's settings. `None` for a caller that isn't a chat's agent. A chat whose mode Koyo can't
/// tell counts as asking first.
fn caller_mode(
    caller: &Caller,
    chats: &Entity<Chats>,
    cx: &mut App,
) -> Option<impl Future<Output = Mode> + use<>> {
    let id = ThreadId(caller.chat.clone()?);
    let summary = chats.read(cx).summary(&id).cloned();
    let chosen = summary
        .as_ref()
        .map(Settings::of_summary)
        .unwrap_or_default();
    let asking = (summary.filter(|_| chosen.permission.is_none())).map(|summary| {
        chats.update(cx, |chats, cx| {
            chats.ask_catalog(Path::new(&summary.cwd), cx)
        })
    });
    let asking = OptionFuture::from(asking);
    Some(async move {
        let default = (asking.await)
            .and_then(Result::ok)
            .and_then(|catalog| catalog.default_mode);
        chosen.mode_by(default)
    })
}

/// Asks what the chat `input` names is running to stop.
pub(crate) fn stop(input: &ChatInput, cx: &mut App) -> Result<Value, ControlError> {
    let id = ThreadId(input.chat.clone());
    known(&id, cx)?
        .0
        .update(cx, |chats, cx| chats.stop(&id, cx));
    output(input)
}

/// The chats and what they know of `id`, if it's one of them. A subagent's isn't: it's its
/// parent's to run.
fn known(id: &ThreadId, cx: &App) -> Result<(Entity<Chats>, ThreadSummary), ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let chats = shell.read(cx).chats().clone();
    let summary = chats.read(cx).summary(id).cloned().ok_or_else(|| {
        ControlError::not_found(format!(
            "No chat has the id `{id}`. `chat_list` lists them."
        ))
    })?;
    Ok((chats, summary))
}

fn prompt(text: String) -> Result<Prompt, ControlError> {
    if text.trim().is_empty() {
        return Err(ControlError::invalid_input(
            "There's nothing in `text` to say.",
        ));
    }
    Ok(Prompt {
        message_id: MessageId::fresh(),
        text,
        attachments: Vec::new(),
    })
}

fn refused(what: &str, error: &ClientError) -> ControlError {
    ControlError::internal(format!("Koyo couldn't {what}: {error}"))
}
