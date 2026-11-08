//! The activity log (`docs/app-control.md` 4, 5): every changing call, refused or not, and
//! every decision the person made about a client or a destructive call, the latest
//! [`KEPT`], kept in memory while the app runs. Reads aren't logged. Listing it is a read;
//! an external client lists its own entries, an agent Koyo runs every client's.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::count;
use crate::{ErrorCode, Hints, Operation, Scope};

/// How many entries the log keeps: the most `activity_list` lists.
pub const KEPT: u32 = 200;
/// How many entries `activity_list` lists when its `limit` is left out.
pub const DEFAULT_LIMIT: u32 = 50;

/// How much of the log to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityListInput {
    /// At most this many entries, newest first: 50 when left out, at most 200.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl ActivityListInput {
    /// How many entries to list, within what the log keeps.
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).min(KEPT) as usize
    }
}

/// What an entry tells of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// A changing call, whether it succeeded, failed or was refused.
    Call,
    /// The person was asked to allow the client to control Koyo.
    Approval,
    /// The person was asked to confirm a destructive call.
    Confirmation,
}

/// What the person answered when asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActivityDecision {
    /// Allowed the client, or confirmed the call.
    Allowed,
    /// Denied the client, or cancelled the call.
    Denied,
    /// Didn't answer in time, which counts as denied.
    TimedOut,
}

/// Why a call failed or was refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityError {
    /// The error's code, as the call answered it.
    pub code: ErrorCode,
    /// What the call was told.
    pub message: String,
}

/// One entry of the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// When, in milliseconds since the Unix epoch.
    pub at: u64,
    /// Who: the name a client said hello with, `terminal:<handle>` for an agent in a Koyo
    /// terminal, or `chat:<id>` for a chat's agent.
    pub client: String,
    /// `call`, `approval` or `confirmation`.
    pub kind: ActivityKind,
    /// The operation called, or the one the person was asked to confirm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// A call that succeeded: the one-line summary of what it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// A call that failed or was refused: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ActivityError>,
    /// A confirmation: what the person was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// An approval or a confirmation: what the person answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<ActivityDecision>,
}

/// The entries `activity_list` found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityListOutput {
    /// The entries, newest first.
    pub entries: Vec<ActivityEntry>,
    /// Whether only your own entries are listed: an external client lists its own, an agent
    /// in a Koyo terminal or chat every client's.
    pub only_yours: bool,
}

/// `activity_list`: what clients did lately, and what the person answered them.
#[derive(Debug)]
pub struct ActivityList;

impl Operation for ActivityList {
    const NAME: &'static str = "activity_list";
    const TITLE: &'static str = "List recent activity";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "What clients did in Koyo lately, newest first: every changing call (refused or not) with its summary or error, and every time the person was asked to allow a client or confirm a destructive call, with their answer. Kept in memory while the app runs, the latest 200; reads aren't logged. An agent in a Koyo terminal or chat lists every client's entries; any other client lists its own (`onlyYours`).";
    const HINTS: Hints = Hints::READ;
    type Input = ActivityListInput;
    type Output = ActivityListOutput;

    fn summary(output: &ActivityListOutput) -> String {
        let entries = count(output.entries.len(), "activity entry", "activity entries");
        if output.only_yours {
            format!("{entries} of yours")
        } else {
            entries
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_limit_defaults_to_fifty_and_stops_at_what_the_log_keeps() {
        assert_eq!(ActivityListInput::default().limit(), 50);
        assert_eq!(ActivityListInput { limit: Some(3) }.limit(), 3);
        assert_eq!(ActivityListInput { limit: Some(0) }.limit(), 0);
        assert_eq!(ActivityListInput { limit: Some(5000) }.limit(), 200);
    }

    #[test]
    fn entries_carry_only_what_their_kind_has() {
        let output = ActivityListOutput {
            entries: vec![
                ActivityEntry {
                    at: 2,
                    client: "terminal:term_1".into(),
                    kind: ActivityKind::Call,
                    operation: Some("tab_open".into()),
                    summary: None,
                    error: Some(ActivityError {
                        code: ErrorCode::ScopeDenied,
                        message: "Not allowed.".into(),
                    }),
                    question: None,
                    decision: None,
                },
                ActivityEntry {
                    at: 1,
                    client: "Claude Code".into(),
                    kind: ActivityKind::Approval,
                    operation: None,
                    summary: None,
                    error: None,
                    question: None,
                    decision: Some(ActivityDecision::TimedOut),
                },
            ],
            only_yours: false,
        };
        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            json!({
                "entries": [
                    {
                        "at": 2, "client": "terminal:term_1", "kind": "call",
                        "operation": "tab_open",
                        "error": { "code": "scope_denied", "message": "Not allowed." }
                    },
                    { "at": 1, "client": "Claude Code", "kind": "approval", "decision": "timed_out" }
                ],
                "onlyYours": false
            })
        );
        assert_eq!(ActivityList::summary(&output), "2 activity entries");
        let yours = ActivityListOutput {
            entries: output.entries[..1].to_vec(),
            only_yours: true,
        };
        assert_eq!(ActivityList::summary(&yours), "1 activity entry of yours");
    }
}
