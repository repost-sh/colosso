//! What the host tells the app without being asked (`docs/chat.md` 4.3.4), as
//! `koyo-js/koyo-agent-host/src/protocol/events.ts` has it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

use super::ids::{ItemId, ThreadId};
use super::limits::AccountLimits;
use super::model::{Item, NoticeLevel, Run, Thread, ThreadSummary};
use super::requests::Request;

macro_rules! events {
    ($($(#[$doc:meta])* $name:literal => $event:ident($params:ty);)*) => {
        /// An event, by its name, with its params.
        #[derive(Clone, Debug, PartialEq, Serialize)]
        #[serde(tag = "event", content = "params")]
        pub enum Event {
            $(
                $(#[$doc])*
                #[serde(rename = $name)]
                $event($params),
            )*
            /// An event this build doesn't know, from a newer host.
            #[serde(untagged)]
            Unknown { event: String, params: Value },
        }

        impl Event {
            /// The event `name` with its `params`, as a line has them.
            pub fn read(name: &str, params: &RawValue) -> serde_json::Result<Event> {
                let params = params.get();
                Ok(match name {
                    $($name => Event::$event(serde_json::from_str(params)?),)*
                    _ => Event::Unknown {
                        event: name.to_owned(),
                        params: serde_json::from_str(params)?,
                    },
                })
            }
        }
    };
}

events! {
    /// A thread as lists show it, to a client that watches threads, when what they show
    /// changed.
    "thread.summary" => ThreadSummary(ThreadSummary);
    /// A thread deleted, to a client that watches threads.
    "thread.removed" => ThreadRemoved(Removed);
    /// The rows one step of a thread's actor changed, whole, to a client that opened it.
    "thread.changed" => ThreadChanged(Box<Changed>);
    /// Text a streaming item grew by, in order after the change that made the item.
    "item.appended" => ItemAppended(Appended);
    "host.notice" => HostNotice(HostNotice);
    /// What a chat's response said of the account's limits, to a client that watches threads.
    "account.limits" => AccountLimits(AccountLimits);
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Removed {
    pub thread_id: ThreadId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub thread_id: ThreadId,
    /// The thread's sequence after the step, which a reopen passes as its `sinceSeq`.
    pub seq: u64,
    /// The thread itself, only when the step changed it.
    pub thread: Option<Thread>,
    pub runs: Vec<Run>,
    pub items: Vec<Item>,
    pub requests: Vec<Request>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appended {
    pub thread_id: ThreadId,
    pub item_id: ItemId,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostNotice {
    pub level: NoticeLevel,
    pub message: String,
}
