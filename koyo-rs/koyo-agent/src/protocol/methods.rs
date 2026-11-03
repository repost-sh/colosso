//! What the app asks of the host (`docs/chat.md` 4.3): each method with the shape of its
//! params and of its result, as `koyo-js/koyo-agent-host/src/protocol/methods.ts` has them.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use super::ids::{RequestId, RunId, ThreadId};
use super::model::{Attachment, Effort, Item, Mode, Prompt, Run, Thread, ThreadSummary};
use super::requests::{Request, Response};

/// A method of the host's: its name, and the shapes of its params and its result.
pub trait Method {
    const NAME: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

macro_rules! methods {
    ($($(#[$doc:meta])* $name:literal => $method:ident($params:ty) -> $result:ty;)*) => {
        $(
            $(#[$doc])*
            pub enum $method {}

            impl Method for $method {
                const NAME: &'static str = $name;
                type Params = $params;
                type Result = $result;
            }
        )*

        /// The name of every method.
        pub const METHODS: &[&str] = &[$($name),*];
    };
}

methods! {
    "hello" => Hello(HelloParams) -> HelloResult;
    "shutdown" => Shutdown(Empty) -> Empty;
    /// Koyo's settings of the agents.
    "config.set" => ConfigSet(Config) -> Empty;
    "providers.status" => ProvidersStatus(StatusParams) -> Vec<ProviderStatus>;
    "catalog" => GetCatalog(CatalogParams) -> Catalog;
    /// Every thread as lists show it, then `thread.summary` for each change.
    "threads.watch" => ThreadsWatch(Empty) -> Vec<ThreadSummary>;
    "threads.create" => ThreadsCreate(CreateParams) -> Thread;
    /// The thread's latest runs, or what changed since `since_seq`, then its changes.
    "threads.open" => ThreadsOpen(OpenParams) -> ThreadSnapshot;
    "threads.history" => ThreadsHistory(HistoryParams) -> Rows;
    "threads.close" => ThreadsClose(ThreadParams) -> Empty;
    "threads.update" => ThreadsUpdate(UpdateParams) -> Thread;
    /// The person has seen the thread.
    "threads.read" => ThreadsRead(ThreadParams) -> Empty;
    "threads.delete" => ThreadsDelete(ThreadParams) -> Empty;
    "turns.send" => TurnsSend(SendParams) -> Sent;
    "turns.stop" => TurnsStop(StopParams) -> Empty;
    "turns.compact" => TurnsCompact(ThreadParams) -> Compacting;
    "queue.remove" => QueueRemove(QueuedParams) -> Empty;
    "queue.edit" => QueueEdit(EditParams) -> Empty;
    "queue.sendNow" => QueueSendNow(QueuedParams) -> SentNow;
    /// Releases a held queue.
    "queue.resume" => QueueResume(ThreadParams) -> Empty;
    "requests.respond" => RequestsRespond(RespondParams) -> Empty;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    App,
    Cli,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub kind: ClientKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    pub client: ClientInfo,
    pub version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloResult {
    pub version: u32,
    pub pid: u32,
    /// A run is live or a request waits, so another build waits to replace it.
    pub busy: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub claude_path: Option<String>,
    pub default_mode: Option<Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusParams {
    pub refresh: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Ready,
    Missing,
    SignedOut,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub email: Option<String>,
    pub plan: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: String,
    pub state: ProviderState,
    pub executable: Option<String>,
    pub version: Option<String>,
    pub account: Option<Account>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogParams {
    pub provider: String,
    pub cwd: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub description: String,
    pub efforts: Vec<Effort>,
    pub fast: bool,
    pub auto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub models: Vec<Model>,
    pub commands: Vec<Command>,
    /// The mode a chat that names none runs in, from the provider's settings.
    pub default_mode: Option<Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateParams {
    pub cwd: String,
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub permission: Option<Mode>,
    pub plan: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadParams {
    pub thread_id: ThreadId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    pub thread_id: ThreadId,
    pub since_seq: Option<u64>,
}

/// Runs with their timeline entries and requests.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rows {
    pub runs: Vec<Run>,
    pub items: Vec<Item>,
    pub requests: Vec<Request>,
}

/// A thread with its latest runs' rows, or with the rows changed after `since_seq`. The rows
/// are written out rather than a flattened [`Rows`], which serde would buffer whole first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadSnapshot {
    pub thread: Thread,
    pub runs: Vec<Run>,
    pub items: Vec<Item>,
    pub requests: Vec<Request>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryParams {
    pub thread_id: ThreadId,
    /// The page is the runs sent before this one.
    pub before: RunId,
    pub runs: u32,
}

/// A change to a thread: a field left `None` stays as it is; `Some(None)` clears it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::option_option,
    reason = "left out keeps a setting, null resets it to Koyo's, a value sets it"
)]
pub struct UpdateParams {
    pub thread_id: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "given"
    )]
    pub model: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "given"
    )]
    pub effort: Option<Option<Effort>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "given"
    )]
    pub permission: Option<Option<Mode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<bool>,
}

/// A field that's there, `null` included: absent is the default, `None`.
fn given<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// Starts a turn, or joins the running one.
    Auto,
    /// Waits for the running turn to end.
    Queue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendParams {
    pub thread_id: ThreadId,
    pub prompt: Prompt,
    pub delivery: Delivery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveredAs {
    Started,
    Queued,
    Steered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub run_id: RunId,
    pub delivered: DeliveredAs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopParams {
    pub thread_id: ThreadId,
    /// Ends the agent's process rather than asking it to stop.
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Compacting {
    pub run_id: RunId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedParams {
    pub thread_id: ThreadId,
    pub run_id: RunId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditParams {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SentNow {
    pub delivered: DeliveredAs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RespondParams {
    pub thread_id: ThreadId,
    pub request_id: RequestId,
    pub response: Response,
}
