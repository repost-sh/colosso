//! What a chat runs with, and what a worktree's chats can choose from: the model, how hard it
//! works, how much it may do without asking, its fast mode and planning first, as the composer's
//! chips set them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Hints, Operation, Scope};

/// How much a chat's agent may do without asking a person, planning aside.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ChatMode {
    /// It asks before it changes anything.
    Ask,
    /// It changes files without asking, and asks before anything else.
    AcceptEdits,
    /// It goes ahead where a classifier judges it safe, and asks otherwise; only on a model that
    /// can.
    Auto,
    /// It does anything without asking.
    FullAccess,
}

impl ChatMode {
    /// Every mode, from the one that asks the most.
    pub const ALL: [ChatMode; 4] = [
        ChatMode::Ask,
        ChatMode::AcceptEdits,
        ChatMode::Auto,
        ChatMode::FullAccess,
    ];

    /// Its name on the wire.
    pub fn id(self) -> &'static str {
        match self {
            ChatMode::Ask => "ask",
            ChatMode::AcceptEdits => "accept_edits",
            ChatMode::Auto => "auto",
            ChatMode::FullAccess => "full_access",
        }
    }
}

/// How hard a chat's agent works.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ChatEffort {
    /// The least.
    Low,
    /// Less than most.
    Medium,
    /// Hard.
    High,
    /// Extra high.
    Xhigh,
    /// The most.
    Max,
}

impl ChatEffort {
    /// Its name on the wire.
    pub fn id(self) -> &'static str {
        match self {
            ChatEffort::Low => "low",
            ChatEffort::Medium => "medium",
            ChatEffort::High => "high",
            ChatEffort::Xhigh => "xhigh",
            ChatEffort::Max => "max",
        }
    }
}

/// What a chat runs with, as its composer shows it. What the chat didn't choose is what the
/// person's settings and the agent's own choose for it, and follows them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatRunsWith {
    /// The model's id, as `chat_choices` lists it, or as the agent's own settings name it. Left
    /// out while Koyo can't tell: the agent isn't ready, or Koyo couldn't ask it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// What the model goes by, as the composer says it: "Opus 5.5". Left out for a model Koyo
    /// doesn't list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
    /// How hard the agent works. Left out on a model with no efforts to choose, or while Koyo
    /// can't tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ChatEffort>,
    /// How much the agent may do without asking. Left out while Koyo can't tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ChatMode>,
    /// Whether the model runs in its fast mode.
    pub fast: bool,
    /// Whether the agent plans first, and asks a person to accept the plan before it acts.
    pub plan: bool,
}

impl ChatRunsWith {
    /// `Opus 5.5 · xhigh · full_access`, as a summary says it.
    pub fn summary(&self) -> String {
        let model = (self.model_name.as_deref()).or(self.model.as_deref());
        let parts = [
            model,
            self.effort.map(ChatEffort::id),
            self.mode.map(ChatMode::id),
            self.fast.then_some("fast"),
            self.plan.then_some("plan first"),
        ];
        parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
    }
}

/// The worktree whose chats' choices to read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatChoicesInput {
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
}

/// A model a chat can run on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatModelChoice {
    /// Its id, for `chat_create`'s `model`.
    pub id: String,
    /// What it goes by, as the composer says it once it's chosen.
    pub name: String,
    /// What its row in the composer's menu says, where that's more than its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    /// The efforts it can run at, from the least; none when it has none to choose.
    pub efforts: Vec<ChatEffort>,
    /// Whether it has a fast mode.
    pub fast: bool,
    /// Whether it can run in the `auto` mode.
    pub auto: bool,
}

/// What a worktree's chats can choose from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatChoices {
    /// The models, in the order the agent lists them. None while the agent isn't ready.
    pub models: Vec<ChatModelChoice>,
    /// The modes, from the one that asks the most; `auto` only on a model that can.
    pub modes: Vec<ChatMode>,
    /// What a chat `chat_create` starts runs with for what the call leaves out.
    pub defaults: ChatRunsWith,
}

/// `chat_choices`: what a worktree's chats can be set to, and what a new one gets.
#[derive(Debug)]
pub struct ChatChoicesRead;

impl Operation for ChatChoicesRead {
    const NAME: &'static str = "chat_choices";
    const TITLE: &'static str = "Chat choices";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "What a worktree's chats can be set to, as the composer \
        offers it: the models, with the efforts each runs at and whether it has a fast mode and \
        the `auto` mode; the modes; and what a chat `chat_create` starts runs with for what the \
        call leaves out.";
    const HINTS: Hints = Hints::READ;
    type Input = ChatChoicesInput;
    type Output = ChatChoices;

    fn summary(output: &ChatChoices) -> String {
        format!(
            "{} models; a new chat runs with {}",
            output.models.len(),
            output.defaults.summary()
        )
    }
}

/// What to change of what a chat runs with: what's left out stays as it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatSetInput {
    /// The chat, by the id `chat_list` lists.
    pub chat: String,
    /// The model to run on, by its id or its name as `chat_choices` lists them. It keeps only
    /// what it can do: the effort it was on or the nearest below, fast if it has it, `auto` if
    /// it can, else `ask`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How hard the agent works, one of its model's efforts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ChatEffort>,
    /// How much the agent may do without asking, granted as asked, `full_access` included, but
    /// for an agent in a Koyo chat no more than its own chat's mode, its own chat included. Ask
    /// for one only as the person did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ChatMode>,
    /// Run the model in its fast mode, if it has one, or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<bool>,
    /// Have the agent plan first, and ask a person to accept the plan before it acts, or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<bool>,
}

/// The chat `chat_set` changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatChanged {
    /// Its id.
    pub chat: String,
    /// What it runs with now, as its composer shows it: what to tell a person it's set to.
    pub runs_with: ChatRunsWith,
}

/// `chat_set`: changes what a chat runs with, as its composer's chips do.
#[derive(Debug)]
pub struct ChatSet;

impl Operation for ChatSet {
    const NAME: &'static str = "chat_set";
    const TITLE: &'static str = "Set chat";
    const SUBJECT: &'static [&'static str] = &["chat"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Changes what a chat runs with, as its composer's chips do: \
        its `model`, `effort` and `mode`, `fast` and `plan`; what's left out stays. \
        `chat_choices` lists what the chat's worktree offers; what its model can't run with is \
        refused, naming what it can. A chat at work takes the change as its session can, by its \
        next turn at the latest. The answer says what the chat runs with now.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ChatSetInput;
    type Output = ChatChanged;

    fn summary(output: &ChatChanged) -> String {
        format!(
            "Chat {} runs on {}",
            output.chat,
            output.runs_with.summary()
        )
    }
}
