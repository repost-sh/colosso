//! The activity log: every changing call, refused or not, and every decision the person made
//! about a client or a destructive call, the latest 200, kept in memory while the app runs.
//! Reads aren't logged. Agents read it with `activity_list` (`exec/activity.rs`).

use std::collections::VecDeque;

use koyo_app_control::ControlError;
/// What the person decided when asked: allowed (or confirmed), denied (or cancelled), or no
/// answer in time, which counts as denied; as `activity_list` says it.
pub use koyo_app_control::ops::activity::ActivityDecision as Decision;
pub(crate) use koyo_term_control::now_ms;

/// How many entries the log keeps, the most `activity_list` lists.
pub(crate) const KEPT: usize = koyo_app_control::ops::activity::KEPT as usize;

/// What happened.
#[derive(Clone, Debug, PartialEq)]
pub enum ActivityKind {
    /// A changing call: its operation, and the one-line summary of its output or why it
    /// failed or was refused.
    Call {
        operation: String,
        outcome: Result<String, ControlError>,
    },
    /// The person was asked to allow a client to control Koyo.
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
    /// The client: its name, `terminal:<handle>` for an agent in a terminal, or `chat:<id>`
    /// for a chat's.
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

    /// The entries, newest first.
    pub fn newest(&self) -> impl Iterator<Item = &ActivityEntry> {
        self.entries.iter().rev()
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
        let entries: Vec<_> = activity.newest().collect();
        assert_eq!(entries.len(), KEPT);
        assert_eq!(entries[0].client, format!("client-{}", KEPT + 4));
        assert_eq!(entries[KEPT - 1].client, "client-5");
    }
}
