//! The chat records the host keeps and sends (`docs/chat.md` 4.4), as the host's zod schemas
//! in `koyo-js/koyo-agent-host/src/protocol/model.ts` define them. A missing value is `null`
//! on the wire, never an absent field. What a newer host may add without the app knowing it
//! (an item's kind, a request's) reads as `Unknown`, so it never breaks the app.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::{ItemId, MessageId, RequestId, RunId, ThreadId, Time};
use super::requests::RequestKind;

/// How much the agent may do without asking (C12), each more than the one before; the plan toggle
/// is separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Ask,
    AcceptEdits,
    Auto,
    FullAccess,
}

impl Mode {
    /// Every mode, from the one that asks the most.
    pub const ALL: [Mode; 4] = [Mode::Ask, Mode::AcceptEdits, Mode::Auto, Mode::FullAccess];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// Every effort, from the least.
    pub const ALL: [Effort; 5] = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ];
}

/// A file the app put in the host's attachments folder, named there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub file: String,
    pub name: String,
    pub mime: String,
}

/// What a person sends: the text and what's attached to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub message_id: MessageId,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleSource {
    Seed,
    Generated,
    User,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the host's record as it's sent, and each of these is its own setting"
)]
pub struct Thread {
    pub id: ThreadId,
    pub cwd: String,
    pub title: String,
    pub title_source: TitleSource,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub fast: bool,
    /// The mode chosen for this chat; `None` leaves it to the provider's own settings.
    pub permission: Option<Mode>,
    pub plan: bool,
    pub session_id: Option<String>,
    /// The entry of the conversation its next session goes on from, after a person went back.
    pub resume_at: Option<String>,
    /// When the chat last wrote files, as far as anyone knows: a later write is someone else's.
    pub files_settled_at: Option<String>,
    /// Queued runs wait for the person after a stop, a failure or a restart.
    pub queue_held: bool,
    /// A tab shows it, and will again after a restart (C13).
    pub tab_open: bool,
    /// The row of the thread that started it, for a subagent's: it runs on its own.
    pub parent: Option<Box<ThreadParent>>,
    /// How many tasks the agent has running in the background, as its session last said: shells,
    /// Monitors, subagents past their turn. It goes on from them as they end, so the chat isn't
    /// done. None while no session runs, and from a host from before it said.
    #[serde(default)]
    pub background_tasks: u64,
    pub created_at: Time,
    pub updated_at: Time,
    pub read_at: Option<Time>,
    pub seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadParent {
    pub thread_id: ThreadId,
    pub item_id: ItemId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Starting,
    Running,
    Stopping,
    Completed,
    Interrupted,
    Failed,
    /// Taken off the queue before it started; shown no more.
    Removed,
}

impl RunStatus {
    /// Whether a session works on the run: past queued, not yet ended.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            RunStatus::Starting | RunStatus::Running | RunStatus::Stopping
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    Api,
    UsageLimit,
    Auth,
    Overloaded,
    Process,
    Host,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    pub retry_at: Option<Time>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cost_usd: Option<f64>,
    /// What the conversation holds now, and what the model can hold.
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoppedBy {
    Person,
    Host,
}

/// Who asked a run to stop, and whether its process was killed rather than asked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub requested_at: Time,
    pub by: StoppedBy,
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub added: u64,
    pub removed: u64,
}

/// A turn: one prompt and the work it caused. A queued run is only its prompt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: RunId,
    pub thread_id: ThreadId,
    pub status: RunStatus,
    pub prompt: Prompt,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    /// The mode in force while it ran, as the provider reported it.
    pub permission: Option<Mode>,
    pub plan: bool,
    pub requested_at: Time,
    pub started_at: Option<Time>,
    pub ended_at: Option<Time>,
    pub stop: Option<Stop>,
    pub failure: Option<Failure>,
    pub usage: Option<Usage>,
    pub files: Vec<FileChange>,
    pub seq: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Declined,
    Interrupted,
}

impl ItemStatus {
    /// Whether it's still going: not started yet, or running.
    pub fn is_open(self) -> bool {
        matches!(self, ItemStatus::Pending | ItemStatus::Running)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFamily {
    Command,
    Read,
    Edit,
    Search,
    Web,
    Mcp,
    Agent,
    Todo,
    Skill,
    Other,
}

/// One hunk of a unified diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    pub status: TodoStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

/// One timeline entry, in its run, at its fixed place in the timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: ItemId,
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub ordinal: u64,
    pub status: ItemStatus,
    pub seq: u64,
    #[serde(flatten)]
    pub body: ItemBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivered {
    /// It started its run.
    Turn,
    /// It joined a running one.
    Steer,
}

/// What an item is, by its `kind`, with its `payload`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum ItemBody {
    UserMessage(UserMessage),
    AssistantText(Text),
    Reasoning(Text),
    Tool(Tool),
    Todos(Todos),
    Request(RequestRef),
    Subagent(Box<Subagent>),
    Compaction(Compaction),
    Notice(Notice),
    /// A kind this build doesn't know, from a newer host; shown as a notice.
    #[serde(untagged)]
    Unknown {
        kind: String,
        payload: Value,
    },
}

impl ItemBody {
    /// The text of what streams: an answer, or the agent's thinking.
    pub fn text(&self) -> Option<&str> {
        match self {
            ItemBody::AssistantText(text) | ItemBody::Reasoning(text) => Some(&text.text),
            _ => None,
        }
    }

    /// The text of what streams, to grow.
    pub fn text_mut(&mut self) -> Option<&mut String> {
        match self {
            ItemBody::AssistantText(text) | ItemBody::Reasoning(text) => Some(&mut text.text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessage {
    pub message_id: MessageId,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub delivered: Delivered,
}

/// Text that streams: an answer, or the agent's thinking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub family: ToolFamily,
    pub label: String,
    pub input: Value,
    /// The end of what it printed or returned, bounded.
    pub output: Option<String>,
    pub exit_code: Option<i64>,
    pub patch: Option<Vec<Hunk>>,
    pub files: Vec<String>,
    /// The MCP server, for an MCP tool.
    pub server: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todos {
    pub todos: Vec<Todo>,
}

/// A request, in its place in the timeline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRef {
    pub request_id: RequestId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub description: String,
    pub agent_type: Option<String>,
    /// What it reported as it ended.
    pub summary: Option<String>,
    /// The thread of its own work, which a row from before they had one doesn't have.
    pub thread_id: Option<ThreadId>,
    /// What it says it's doing, while it runs.
    pub progress: Option<String>,
    pub usage: Option<SubagentUsage>,
    pub started_at: Option<Time>,
    pub ended_at: Option<Time>,
}

/// What a subagent has used so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentUsage {
    pub tokens: u64,
    pub tools: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    pub trigger: CompactionTrigger,
    pub before: Option<u64>,
    pub after: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub level: NoticeLevel,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    Failed,
}

/// A thread as lists show it, with what it needs derived (`docs/chat.md` 4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a list's row as the host sends it, and each of these is its own setting or state"
)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub cwd: String,
    pub title: String,
    pub provider: String,
    pub model: Option<String>,
    /// What the chat chose of the rest of what it runs with, as [`Thread`] has it.
    #[serde(default)]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub fast: bool,
    #[serde(default)]
    pub permission: Option<Mode>,
    #[serde(default)]
    pub plan: bool,
    pub status: Status,
    /// What the oldest pending request asks of the person.
    pub needs: Option<RequestKind>,
    /// Whether the agent runs tasks in the background, as a shell or a Monitor
    /// (`docs/chat.md` 4.5.6).
    pub background: bool,
    pub queued: u64,
    pub unread: bool,
    pub tab_open: bool,
    pub updated_at: Time,
    /// The start of what was said last in it, the person's message or the agent's reply, on one
    /// line, as a tab's row shows it; none before anything was, or from an older host.
    #[serde(default)]
    pub said: Option<String>,
}

/// The answers to a question request, by question id: the chosen labels, or the typed text.
pub type Answers = BTreeMap<String, Vec<String>>;
