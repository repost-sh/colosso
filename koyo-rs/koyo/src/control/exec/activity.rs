//! `activity_list`: the activity log as agents read it, newest first. An agent Koyo runs, in
//! a terminal or a chat, reads every client's entries; any other client reads its own. The
//! clients the person allowed are the control policy's, which no operation reads
//! (`policy.rs`), and an external client's name is all another needs to pass for it.

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::activity::{
    self as op, ActivityError, ActivityListInput, ActivityListOutput,
};
use serde_json::Value;

use super::super::activity::{ActivityEntry, ActivityKind};
use super::super::{Caller, policy};
use super::output;

pub(crate) fn list(
    caller: &Caller,
    input: &ActivityListInput,
    cx: &App,
) -> Result<Value, ControlError> {
    let only_yours = caller.external();
    let key = caller.key();
    let entries = policy::activity(cx)
        .filter(|entry| !only_yours || entry.client == key)
        .take(input.limit())
        .map(entry)
        .collect();
    output(&ActivityListOutput {
        entries,
        only_yours,
    })
}

/// An entry of the log as the operation's output has it.
fn entry(entry: &ActivityEntry) -> op::ActivityEntry {
    let mut listed = op::ActivityEntry {
        at: entry.at,
        client: entry.client.clone(),
        kind: op::ActivityKind::Call,
        operation: None,
        summary: None,
        error: None,
        question: None,
        decision: None,
    };
    match &entry.kind {
        ActivityKind::Call { operation, outcome } => {
            listed.operation = Some(operation.clone());
            match outcome {
                Ok(summary) => listed.summary = Some(summary.clone()),
                Err(error) => {
                    listed.error = Some(ActivityError {
                        code: error.code,
                        message: error.message.clone(),
                    });
                }
            }
        }
        ActivityKind::Approval(decision) => {
            listed.kind = op::ActivityKind::Approval;
            listed.decision = Some(*decision);
        }
        ActivityKind::Confirmation {
            operation,
            question,
            decision,
        } => {
            listed.kind = op::ActivityKind::Confirmation;
            listed.operation = Some(operation.clone());
            listed.question = Some(question.clone());
            listed.decision = Some(*decision);
        }
    }
    listed
}
