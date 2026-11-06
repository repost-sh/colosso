//! When a chat can go back to one of its messages (`docs/chat.md` C11).

use koyo_agent::protocol::ids::{ItemId, ThreadId};
use koyo_agent::protocol::methods::{FilesBack, ThreadSnapshot};
use koyo_agent::protocol::model::{RunStatus, ThreadParent};
use koyo_agent::replica::Replica;

use super::super::rewind::restoring;
use super::super::timeline::can_go_back;
use super::{run, snapshot};

fn with_run(status: RunStatus) -> ThreadSnapshot {
    ThreadSnapshot {
        runs: vec![run("r", status, "Why?", 1)],
        ..snapshot("t", 1, Vec::new())
    }
}

#[test]
fn what_restoring_changes_is_said_by_name_up_to_two_files() {
    let said = |files: &[&str]| {
        restoring(&FilesBack {
            files: files.iter().map(ToString::to_string).collect(),
            insertions: 3,
            deletions: 12,
            blocked: None,
        })
    };
    assert_eq!(said(&["src/a.rs"]), "Restoring changes a.rs (+3 −12).");
    assert_eq!(
        said(&["src/a.rs", "b.md"]),
        "Restoring changes a.rs and b.md (+3 −12)."
    );
    assert_eq!(
        said(&["src/a.rs", "b.md", "c"]),
        "Restoring changes a.rs and 2 other files (+3 −12)."
    );
}

#[test]
fn a_chat_with_nothing_going_on_can_go_back() {
    assert!(can_go_back(&Replica::new(with_run(RunStatus::Completed))));
    assert!(can_go_back(&Replica::new(with_run(RunStatus::Failed))));
}

#[test]
fn a_chat_at_work_or_with_messages_waiting_cant() {
    assert!(!can_go_back(&Replica::new(with_run(RunStatus::Running))));
    assert!(!can_go_back(&Replica::new(with_run(RunStatus::Queued))));
}

#[test]
fn a_subagents_chat_cant() {
    let mut held = with_run(RunStatus::Completed);
    held.thread.parent = Some(Box::new(ThreadParent {
        thread_id: ThreadId("p".into()),
        item_id: ItemId("i".into()),
    }));
    assert!(!can_go_back(&Replica::new(held)));
}
