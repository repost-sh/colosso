//! What the daemon tells every client as it happens, in `Event` frames: a terminal's life
//! (opened, renamed, closed), what its program reported (title, cwd, bell, a notification),
//! what the daemon derived (status, exit) and who holds control. `docs/terminals.md` 5.4.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::protocol::ProtocolError;
use crate::terminal::{Actor, Control, Exit, Handle, Progress, Size, Status, TerminalRecord};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub handle: Handle,
    pub kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Opened {
        terminal: Box<TerminalRecord>,
    },
    Renamed {
        name: String,
        title: Option<String>,
        labels: BTreeSet<String>,
    },
    Closed,
    /// The program's process started, again when `incarnation` is above 1.
    Spawned {
        pid: u32,
        incarnation: u32,
    },
    Exited {
        exit: Exit,
    },
    Title {
        title: String,
    },
    /// OSC 7: `outside_worktree` when the shell left the terminal's checkout.
    Cwd {
        cwd: PathBuf,
        outside_worktree: bool,
    },
    Status {
        status: Status,
        reason: String,
        progress: Option<Progress>,
    },
    Control {
        change: ControlChange,
        control: Control,
    },
    /// The terminal's one size changed, so other clients letterbox.
    Size {
        size: Size,
    },
    /// OSC 9 or 777: a program asks for attention.
    Notification {
        title: String,
        body: String,
    },
    Bell,
    /// Only to the client whose `Input` or `Paste` the daemon refused, since those frames
    /// carry no request number to answer.
    Refused {
        error: ProtocolError,
    },
}

/// What happened to control, for the log and the UI's toast.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum ControlChange {
    Claimed {
        by: Actor,
    },
    Released {
        by: Actor,
    },
    /// An agent took it from another, who is told.
    Taken {
        by: Actor,
        from: Actor,
        reason: Option<String>,
    },
    /// A person took it, by typing (`implicit`) or by asking.
    HumanTook {
        by: Actor,
        implicit: bool,
    },
    HumanReleased {
        by: Actor,
    },
    /// An agent's lease lapsed or its connection closed.
    Expired {
        was: Actor,
    },
}
