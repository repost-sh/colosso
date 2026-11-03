//! The ids of the host's records, each kind its own type so a run's id can't be passed where a
//! thread's is wanted. The host makes them (version 7 UUIDs, which sort as they were made);
//! the app only makes message ids, so a send it repeats is recognised (`docs/chat.md` 4.3.3).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    )*};
}

ids! {
    ThreadId;
    RunId;
    ItemId;
    RequestId;
    /// The id the app gives a message it sends.
    MessageId;
}

impl MessageId {
    /// The id of a message about to be sent.
    pub fn fresh() -> MessageId {
        MessageId(uuid::Uuid::now_v7().to_string())
    }
}

/// A time as the host writes it: ISO 8601 in UTC, to the millisecond.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Time(pub String);
