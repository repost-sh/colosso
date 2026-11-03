//! How the catalog's answers look to agents (`docs/terminals.md` 8.5): camelCase, times in
//! Unix milliseconds, actors as `{ kind, id, label }`.

use serde_json::{Value, json};

use crate::{Actor, Control, Controller, Pinned, TerminalRecord, TerminalSummary, WorktreeSummary};

pub fn summary(summary: &TerminalSummary) -> Value {
    record(&summary.record, &summary.preview)
}

pub fn record(record: &TerminalRecord, preview: &str) -> Value {
    json!({
        "terminal": record.handle,
        "name": record.name,
        "title": record.title,
        "worktree": record.worktree.as_ref().map(|worktree| json!({
            "path": worktree.path,
            "branch": worktree.branch,
            "project": worktree.repo_main,
        })),
        "cwd": record.cwd,
        "outsideWorktree": record.outside_worktree(),
        "command": record.command,
        "status": record.status,
        "statusReason": record.status_reason,
        "lastExit": record.last_exit,
        "controller": record.control.holder.as_ref().map(Controller::actor),
        "observers": record.control.observers.len(),
        "attachedClients": record.attached_clients,
        "lastOutputAt": record.last_output_ms,
        "createdAt": record.created_ms,
        "createdBy": record.created_by,
        "labels": record.labels,
        "pinned": record.pinned.as_ref().map(pinned),
        "preview": preview,
    })
}

/// A pinned size and who pinned it (`docs/responsive.md` 4.2).
pub fn pinned(pinned: &Pinned) -> Value {
    json!({ "cols": pinned.cols, "rows": pinned.rows, "by": pinned.by })
}

/// `ControlState`: who holds the terminal, who watches, and whether it's `me`.
pub fn control(control: &Control, me: &Actor) -> Value {
    let holder = control.holder.as_ref().map(|holder| match holder {
        Controller::Human {
            actor,
            since_ms,
            implicit,
        } => json!({
            "kind": "human",
            "id": actor.id,
            "label": actor.label,
            "since": since_ms,
            "implicit": implicit,
        }),
        Controller::Agent {
            actor,
            since_ms,
            lease_until_ms,
            reason,
        } => json!({
            "kind": "agent",
            "id": actor.id,
            "label": actor.label,
            "since": since_ms,
            "leaseUntil": lease_until_ms,
            "reason": reason,
        }),
    });
    let you_hold = (control.holder.as_ref()).is_some_and(|holder| holder.actor().id == me.id);
    json!({
        "holder": holder,
        "observers": control.observers,
        "youHold": you_hold,
    })
}

/// Who holds the terminal, in a few words.
pub fn holder(control: &Control, me: &Actor) -> String {
    match &control.holder {
        None => "no one holds it".to_owned(),
        Some(holder) if holder.actor().id == me.id => "you hold it".to_owned(),
        Some(Controller::Human { .. }) => "the person holds it".to_owned(),
        Some(Controller::Agent { actor, .. }) => format!("{} holds it", actor.label),
    }
}

pub fn worktree(summary: &WorktreeSummary) -> Value {
    let worktree = &summary.worktree;
    json!({
        "path": worktree.path,
        "branch": worktree.branch,
        "project": worktree.repo_main,
        "main": worktree.path == worktree.repo_main,
        "terminals": summary.terminals,
        "needsInput": summary.needs_input,
    })
}
