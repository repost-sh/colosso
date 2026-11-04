//! A worktree's chats with an agent: listed, read, shown in a tab, started, told something and
//! stopped. What the agent asks of a person (an approval, a question, a plan) stays theirs to
//! answer, in the window.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{PanePlace, count};
use crate::{Hints, Operation, Scope};

/// Whose chats to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatListInput {
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Every worktree's chats instead of one's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

/// How a chat is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatStatus {
    /// Nothing's running.
    Idle,
    /// The agent is at work.
    Working,
    /// Its latest run failed.
    Failed,
}

/// What a chat waits on a person for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatNeeds {
    /// To allow something the agent wants to do.
    Approval,
    /// To answer what the agent asked.
    Question,
    /// To accept the agent's plan.
    Plan,
}

/// One chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatInfo {
    /// Its id, for the other chat operations.
    pub id: String,
    /// What it's called.
    pub title: String,
    /// The worktree's checkout path.
    pub worktree: String,
    /// How it's doing.
    pub status: ChatStatus,
    /// What it waits on a person for, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<ChatNeeds>,
    /// How many messages wait their turn.
    pub queued: u64,
    /// Whether it has something a person hasn't seen.
    pub unread: bool,
    /// Whether a tab shows it.
    pub open: bool,
    /// When it last changed.
    pub updated_at: String,
}

/// The chats `chat_list` found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatListOutput {
    /// The chats, latest first.
    pub chats: Vec<ChatInfo>,
}

/// `chat_list`: a worktree's chats and how each is doing.
#[derive(Debug)]
pub struct ChatList;

impl Operation for ChatList {
    const NAME: &'static str = "chat_list";
    const TITLE: &'static str = "List chats";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A worktree's chats with an agent (or every worktree's, with \
        `all`), latest first: each one's id, title, how it's doing and what it waits on a person \
        for, whether a tab shows it or not.";
    const HINTS: Hints = Hints::READ;
    type Input = ChatListInput;
    type Output = ChatListOutput;

    fn summary(output: &ChatListOutput) -> String {
        count(output.chats.len(), "chat", "chats")
    }
}

/// The chat to read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatReadInput {
    /// The chat, by the id `chat_list` lists.
    pub chat: String,
    /// How many of its latest turns to read, from 1; 5 when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<u32>,
}

/// Who said something in a chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    /// A person, or a caller like this one.
    User,
    /// The agent.
    Assistant,
}

/// Something said in a chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Who said it.
    pub role: ChatRole,
    /// What they said.
    pub text: String,
}

/// What `chat_read` found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatReadOutput {
    /// How it's doing: what the agent is writing may not all be here while it's `working`.
    pub status: ChatStatus,
    /// What was said, oldest first. The agent's work between its words isn't here.
    pub messages: Vec<ChatMessage>,
}

/// `chat_read`: what was said in a chat lately.
#[derive(Debug)]
pub struct ChatRead;

impl Operation for ChatRead {
    const NAME: &'static str = "chat_read";
    const TITLE: &'static str = "Read chat";
    const SUBJECT: &'static [&'static str] = &["chat"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "What a person and the agent said in a chat's latest turns, \
        oldest first, without the agent's work between its words; whether a tab shows the chat or \
        not.";
    const HINTS: Hints = Hints::READ;
    type Input = ChatReadInput;
    type Output = ChatReadOutput;

    fn summary(output: &ChatReadOutput) -> String {
        count(output.messages.len(), "message", "messages")
    }
}

/// The chat acted on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatInput {
    /// The chat, by the id `chat_list` lists.
    pub chat: String,
}

/// `chat_open`: shows a chat in its tab, or a new one.
#[derive(Debug)]
pub struct ChatOpen;

impl Operation for ChatOpen {
    const NAME: &'static str = "chat_open";
    const TITLE: &'static str = "Open chat";
    const SUBJECT: &'static [&'static str] = &["chat"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Shows a chat: the tab showing it already, or a new tab on \
        its worktree, as Recent chats in the new-tab menu does.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ChatInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Showing {}", output.summary())
    }
}

/// The chat to start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatCreateInput {
    /// The worktree it works in: a checkout path, a branch name, or `current`. Left out, yours,
    /// else the selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// What to say to the agent first.
    pub text: String,
    /// Show the new tab; true when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

/// The chat `chat_create` started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatCreated {
    /// Its id, for the other chat operations.
    pub chat: String,
    /// The tab that shows it.
    pub place: PanePlace,
}

/// `chat_create`: a new chat in a tab, told something.
#[derive(Debug)]
pub struct ChatCreate;

impl Operation for ChatCreate {
    const NAME: &'static str = "chat_create";
    const TITLE: &'static str = "Start chat";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Starts a chat with the agent in a worktree, set as a new \
        chat is in Settings, in a tab of its own, and says `text` to it. The agent then works in \
        that checkout, as far as its permissions let it.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = ChatCreateInput;
    type Output = ChatCreated;

    fn summary(output: &ChatCreated) -> String {
        format!("Started chat {} in {}", output.chat, output.place.summary())
    }
}

/// What to say to a chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatSendInput {
    /// The chat, by the id `chat_list` lists.
    pub chat: String,
    /// What to say to the agent.
    pub text: String,
    /// Wait for the turn that's running to end, rather than join it. False when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<bool>,
}

/// What `chat_send` said.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatSent {
    /// The turn it's part of.
    pub turn: String,
}

/// `chat_send`: says something to a chat's agent.
#[derive(Debug)]
pub struct ChatSend;

impl Operation for ChatSend {
    const NAME: &'static str = "chat_send";
    const TITLE: &'static str = "Say to chat";
    const SUBJECT: &'static [&'static str] = &["chat"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Says `text` to a chat's agent, as a person would from its \
        composer: it starts a turn, or joins the one running unless `queue` has it wait. The \
        agent then works in that checkout, as far as its permissions let it.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = ChatSendInput;
    type Output = ChatSent;

    fn summary(output: &ChatSent) -> String {
        format!("Said in turn {}", output.turn)
    }
}

/// `chat_stop`: asks what a chat's running to stop.
#[derive(Debug)]
pub struct ChatStop;

impl Operation for ChatStop {
    const NAME: &'static str = "chat_stop";
    const TITLE: &'static str = "Stop chat";
    const SUBJECT: &'static [&'static str] = &["chat"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Asks what a chat's running to stop, as its stop button does. \
        Messages waiting their turn stay, held for a person.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ChatInput;
    type Output = ChatInput;

    fn summary(output: &ChatInput) -> String {
        format!("Asked chat {} to stop", output.chat)
    }
}
