//! The activity log: every changing call, refused or not, and every decision the person made
//! about a client or a destructive call, the latest 200, kept in memory while the app runs.
//! Reads aren't logged.

use std::collections::VecDeque;

use colosso_app_control::ControlError;
pub(crate) use colosso_term_control::now_ms;

/// How many entries the log keeps.
pub(crate) const KEPT: usize = 200;

/// What the person decided when asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Allowed the client, or confirmed the call.
    Allowed,
    /// Denied the client, or cancelled the call.
    Denied,
    /// Didn't answer in time, which counts as denied.
    TimedOut,
}

/// What happened.
#[derive(Clone, Debug, PartialEq)]
pub enum ActivityKind {
    /// A changing call: its operation, and the one-line summary of its output or why it
    /// failed or was refused.
    Call {
        operation: String,
        outcome: Result<String, ControlError>,
    },
    /// The person was asked to allow a client to control Colosso.
    Approval(Decision),
    /// The person was asked to confirm a destructive call: its operation, and what it would
    /// have done.
    Confirmation {
        operation: String,
        question: String,
        decision: Decision,
    },
}

/// One entry of the log.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivityEntry {
    /// When, in milliseconds since the Unix epoch.
    pub at: u64,
    /// The client: its name, or `terminal:<handle>` for an agent in a terminal.
    pub client: String,
    pub kind: ActivityKind,
}

/// The log, oldest entry first.
#[derive(Default)]
pub(crate) struct Activity {
    entries: VecDeque<ActivityEntry>,
}

impl Activity {
    /// Adds `kind` for `client`, letting the oldest entry go past [`KEPT`].
    pub fn record(&mut self, client: String, kind: ActivityKind) {
        if self.entries.len() == KEPT {
            self.entries.pop_front();
        }
        self.entries.push_back(ActivityEntry {
            at: now_ms(),
            client,
            kind,
        });
    }

    /// The latest `count` entries, newest first.
    pub fn entries(&self, count: usize) -> Vec<ActivityEntry> {
        self.entries.iter().rev().take(count).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_entries_newest_first() {
        let mut activity = Activity::default();
        for n in 0..KEPT + 5 {
            activity.record(
                format!("client-{n}"),
                ActivityKind::Approval(Decision::Allowed),
            );
        }
        let entries = activity.entries(usize::MAX);
        assert_eq!(entries.len(), KEPT);
        assert_eq!(entries[0].client, format!("client-{}", KEPT + 4));
        assert_eq!(entries[KEPT - 1].client, "client-5");
    }
}
