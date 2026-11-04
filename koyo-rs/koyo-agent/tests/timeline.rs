//! Which rows a thread's records make: each rule of the timeline, with no drawing.

#[path = "support/records.rs"]
mod records;

use std::collections::HashSet;

use koyo_agent::protocol::ids::{ItemId, RequestId, RunId, ThreadId, Time};
use koyo_agent::protocol::model::{
    Compaction, CompactionTrigger, Effort, FileChange, Item, ItemBody, ItemStatus, Notice,
    NoticeLevel, RequestRef, Run, RunStatus, Subagent, Text, Tool, ToolFamily,
};
use koyo_agent::protocol::requests::{Request, RequestPayload, RequestStatus};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind, changed, children, rows};
use koyo_agent::words::{
    answer_meta, compacted, duration, live_line, tool_line, work_line, work_summary,
};
use records::{THREAD, answer, item, message, run, snapshot};
use serde_json::json;

fn tool_of(family: ToolFamily, label: &str) -> Tool {
    Tool {
        name: "Tool".into(),
        family,
        label: label.into(),
        input: json!({}),
        output: None,
        exit_code: None,
        patch: None,
        files: Vec::new(),
        server: None,
    }
}

fn subagent(description: &str) -> Subagent {
    Subagent {
        description: description.into(),
        agent_type: None,
        summary: None,
    }
}

fn reasoning(said: &str) -> ItemBody {
    ItemBody::Reasoning(Text { text: said.into() })
}

fn tool(label: &str) -> ItemBody {
    ItemBody::Tool(tool_of(ToolFamily::Read, label))
}

/// `body` at `ordinal` in run `r1`, done.
fn done(id: &str, ordinal: u64, body: ItemBody) -> Item {
    Item {
        status: ItemStatus::Completed,
        ..item(id, ordinal, body, ordinal)
    }
}

fn replica(runs: Vec<Run>, items: Vec<Item>) -> Replica {
    Replica::new(snapshot(9, runs, items))
}

/// Each row in a word, with the ids of the items it shows.
fn shape(replica: &Replica, rows: &[Row]) -> Vec<String> {
    let id = |at: &usize| replica.items[*at].id.0.clone();
    (rows.iter())
        .map(|row| match &row.kind {
            RowKind::User { item, .. } => format!("user {}", id(item)),
            RowKind::Answer { item, last, .. } => {
                format!("{} {}", if *last { "answer" } else { "text" }, id(item))
            }
            RowKind::Work { items } => {
                format!("work {}", items.iter().map(id).collect::<Vec<_>>().join(","))
            }
            RowKind::Live { .. } => "live".to_owned(),
            RowKind::Fold { .. } => "fold".to_owned(),
            RowKind::Request { item, .. } => format!("request {}", id(item)),
            RowKind::Compaction { item } => format!("compaction {}", id(item)),
            RowKind::Notice { item } => format!("notice {}", id(item)),
            RowKind::Failure { .. } => "failure".to_owned(),
            RowKind::Stopped { .. } => "stopped".to_owned(),
            RowKind::Files { .. } => "files".to_owned(),
            RowKind::Queued { run } => format!("queued {}", replica.runs[*run].id.0),
        })
        .collect()
}

/// The rows of `replica`, each run folded.
fn made(replica: &Replica) -> Vec<Row> {
    rows(replica, |_| false)
}

fn of(replica: &Replica) -> Vec<String> {
    shape(replica, &made(replica))
}

fn request(id: &str, status: RequestStatus) -> Request {
    Request {
        id: RequestId(id.into()),
        thread_id: ThreadId(THREAD.into()),
        run_id: RunId("r1".into()),
        status,
        payload: RequestPayload::Plan {
            markdown: "# Plan".into(),
        },
        answer: None,
        created_at: Time("2026-11-03T10:00:00.000Z".into()),
        resolved_at: None,
        seq: 3,
    }
}

fn asks(id: &str) -> ItemBody {
    ItemBody::Request(RequestRef {
        request_id: RequestId(id.into()),
    })
}

fn replica_of(status: RunStatus, items: Vec<Item>) -> Replica {
    replica(vec![run("r1", status, 5)], items)
}

#[test]
fn a_message_and_its_answer_are_a_row_each_with_nothing_to_fold() {
    let replica = replica(
        vec![run("r1", RunStatus::Completed, 5)],
        vec![done("a", 0, message("Why?")), done("b", 1, answer("Because."))],
    );
    assert_eq!(of(&replica), ["user a", "answer b"]);
}

#[test]
fn tools_one_after_the_other_are_one_row_that_goes_by_the_first() {
    let replica = replica(
        vec![run("r1", RunStatus::Running, 5)],
        vec![
            done("a", 0, message("Why?")),
            done("b", 1, tool("one")),
            done("c", 2, tool("two")),
            done("d", 3, answer("So far")),
            done("e", 4, tool("three")),
        ],
    );
    let shown = made(&replica);
    assert_eq!(
        shape(&replica, &shown),
        ["user a", "work b,c", "answer d", "work e", "live"]
    );
    assert_eq!(shown[1].id, RowId::Work(ItemId("b".into())));
    assert_eq!(shown[1].seq, 2);
}

#[test]
fn what_is_running_is_the_live_row_and_joins_its_group_when_done() {
    let running = Item {
        status: ItemStatus::Running,
        ..item("c", 2, tool("two"), 2)
    };
    let items = vec![done("b", 1, tool("one")), running.clone()];
    let working = replica(vec![run("r1", RunStatus::Running, 5)], items);
    let shown = made(&working);
    assert_eq!(shape(&working, &shown), ["work b", "live"]);
    assert_eq!(shown[1].kind, RowKind::Live { item: Some(1) });

    let finished = Item {
        status: ItemStatus::Completed,
        seq: 6,
        ..running
    };
    let items = vec![done("b", 1, tool("one")), finished];
    let later = replica_of(RunStatus::Running, items);
    let after = made(&later);
    assert_eq!(shape(&later, &after), ["work b,c", "live"]);
    assert_eq!(after[0].id, shown[0].id, "a group keeps its id as it grows");
    assert_eq!(after[1].id, shown[1].id, "the live row keeps its id");
    assert_eq!(after[0].seq, 6);
}

#[test]
fn a_group_that_starts_running_goes_by_the_same_id_once_it_is_done() {
    let running = Item {
        status: ItemStatus::Running,
        ..item("b", 1, tool("one"), 1)
    };
    let items = vec![running, done("c", 2, tool("two"))];
    let working = replica_of(RunStatus::Running, items);
    let shown = made(&working);
    assert_eq!(shape(&working, &shown), ["work c", "live"]);
    assert_eq!(shown[0].id, RowId::Work(ItemId("b".into())));
}

#[test]
fn a_live_row_shows_unless_what_the_run_does_shows_for_itself() {
    // Nothing yet, not even an item: it still has its row.
    let starting = replica(vec![run("r1", RunStatus::Starting, 5)], Vec::new());
    assert_eq!(made(&starting)[0].kind, RowKind::Live { item: None });

    let text = Item {
        status: ItemStatus::Running,
        ..item("b", 1, answer("So"), 1)
    };
    let writing = replica_of(RunStatus::Running, vec![text]);
    assert_eq!(of(&writing), ["answer b"]);

    let mut snap = snapshot(
        9,
        vec![run("r1", RunStatus::Running, 5)],
        vec![done("b", 1, asks("q1"))],
    );
    snap.requests = vec![request("q1", RequestStatus::Pending)];
    assert_eq!(of(&Replica::new(snap)), ["request b"]);
}

#[test]
fn a_settled_run_folds_its_work_and_keeps_what_was_asked_and_its_answer() {
    let mut snap = snapshot(
        9,
        vec![run("r1", RunStatus::Completed, 5)],
        vec![
            done("a", 0, message("Why?")),
            done("b", 1, tool("one")),
            done("c", 2, answer("Looking")),
            done("d", 3, asks("q1")),
            done("e", 4, ItemBody::Compaction(Compaction {
                trigger: CompactionTrigger::Auto,
                before: None,
                after: None,
            })),
            done("f", 5, tool("two")),
            done("g", 6, answer("Done")),
        ],
    );
    snap.requests = vec![request("q1", RequestStatus::Answered)];
    let replica = Replica::new(snap);
    assert_eq!(
        of(&replica),
        ["user a", "fold", "request d", "compaction e", "answer g"]
    );

    let open = HashSet::from([RunId("r1".into())]);
    assert_eq!(
        shape(&replica, &rows(&replica, |run| open.contains(run))),
        [
            "user a",
            "fold",
            "work b",
            "text c",
            "request d",
            "compaction e",
            "work f",
            "answer g",
        ]
    );
}

#[test]
fn failed_and_interrupted_runs_show_their_work_and_how_they_ended() {
    let items = || vec![done("a", 0, message("Why?")), done("b", 1, tool("one"))];
    assert_eq!(
        of(&replica_of(RunStatus::Failed, items())),
        ["user a", "work b", "failure"]
    );
    assert_eq!(
        of(&replica_of(RunStatus::Interrupted, items())),
        ["user a", "work b", "stopped"]
    );
}

#[test]
fn the_files_a_run_changed_come_under_its_answer() {
    let mut settled = run("r1", RunStatus::Completed, 5);
    settled.files = vec![FileChange {
        path: "src/main.rs".into(),
        added: 2,
        removed: 1,
    }];
    let replica = replica(vec![settled], vec![done("b", 1, answer("Done"))]);
    assert_eq!(of(&replica), ["answer b", "files"]);
}

#[test]
fn a_note_stays_unless_it_is_only_for_information() {
    let note = |level| {
        ItemBody::Notice(Notice {
            level,
            text: "Retrying".into(),
        })
    };
    let replica = replica_of(RunStatus::Completed, vec![
        done("a", 0, note(NoticeLevel::Info)),
        done("b", 1, note(NoticeLevel::Warning)),
        done("c", 2, answer("Done")),
    ]);
    assert_eq!(of(&replica), ["fold", "notice b", "answer c"]);
}

#[test]
fn what_an_agent_did_shows_inside_it_not_as_rows_of_its_own() {
    let child = Item {
        parent_id: Some(ItemId("b".into())),
        ..done("c", 2, tool("inside"))
    };
    let items = vec![done("b", 1, tool("agent")), child, done("d", 3, answer("Done"))];
    let replica = replica_of(RunStatus::Failed, items);
    assert_eq!(of(&replica), ["work b", "answer d", "failure"]);
    let agent = ItemId("b".into());
    let inside: Vec<&str> = children(&replica, &agent).map(|child| child.id.0.as_str()).collect();
    assert_eq!(inside, ["c"]);
}

#[test]
fn queued_runs_come_last_and_removed_ones_not_at_all() {
    let replica = replica(
        vec![
            run("r0", RunStatus::Queued, 1),
            run("r1", RunStatus::Completed, 5),
            run("r2", RunStatus::Removed, 6),
            run("r3", RunStatus::Queued, 7),
        ],
        vec![done("b", 1, answer("Done"))],
    );
    assert_eq!(of(&replica), ["answer b", "queued r0", "queued r3"]);
}

#[test]
fn a_change_replaces_only_the_rows_it_touched() {
    let at = |id: &str, seq| Row {
        id: RowId::Item(ItemId(id.into())),
        seq,
        kind: RowKind::User { item: 0, run: 0 },
    };
    let old = [at("a", 1), at("b", 1), at("c", 1)];
    assert_eq!(changed(&old, &old.clone()), None);
    // A row added at the end.
    assert_eq!(
        changed(&old, &[at("a", 1), at("b", 1), at("c", 1), at("d", 1)]),
        Some((3..3, 1))
    );
    // One changed in the middle.
    assert_eq!(
        changed(&old, &[at("a", 1), at("b", 2), at("c", 1)]),
        Some((1..2, 1))
    );
    // A page of history, which moves everything in the replica and nothing in the list.
    let moved = Row {
        kind: RowKind::User { item: 9, run: 0 },
        ..at("a", 1)
    };
    assert_eq!(
        changed(&old, &[at("z", 1), moved, at("b", 1), at("c", 1)]),
        Some((0..0, 1))
    );
    // Gone from the end.
    assert_eq!(changed(&old, &[at("a", 1)]), Some((1..3, 0)));
    // An answer that stops being the last loses the line under it, with nothing else changed.
    let last = |last| Row {
        kind: RowKind::Answer { item: 0, run: 0, last },
        ..at("a", 1)
    };
    assert_eq!(changed(&[last(true)], &[last(false)]), Some((0..1, 1)));
}

#[test]
fn work_done_together_is_said_in_a_sentence() {
    let of = |family| done("a", 1, ItemBody::Tool(tool_of(family, "a")));
    let (read, ran, web) = (of(ToolFamily::Read), of(ToolFamily::Command), of(ToolFamily::Web));
    let said = |items: &[&Item]| work_summary(items.iter().copied());
    assert_eq!(said(&[&read]), "Read a file");
    assert_eq!(said(&[&read, &read, &read, &ran, &ran]), "Read 3 files and ran 2 commands");
    assert_eq!(said(&[&ran, &read, &web, &web]), "Ran a command, read a file and searched the web");
    let agent = done("b", 1, ItemBody::Subagent(subagent("Check")));
    assert_eq!(said(&[&agent, &agent]), "Ran 2 agents");
    let thought = done("c", 1, reasoning("Hm"));
    assert_eq!(said(&[&thought, &thought]), "Thought");
}

#[test]
fn a_tools_line_says_what_it_did_with_paths_from_the_checkout() {
    let command = tool_of(ToolFamily::Command, "cargo test");
    assert_eq!(tool_line(&command, ItemStatus::Running, "/work"), "Running cargo test");
    assert_eq!(tool_line(&command, ItemStatus::Completed, "/work"), "Ran cargo test");
    let read = tool_of(ToolFamily::Read, "/work/koyo/src/main.rs");
    assert_eq!(tool_line(&read, ItemStatus::Completed, "/work/koyo/"), "Read src/main.rs");
    let search = tool_of(ToolFamily::Search, "fn main in /work/koyo/src");
    assert_eq!(
        tool_line(&search, ItemStatus::Completed, "/work/koyo"),
        "Searched for fn main in src"
    );
}

#[test]
fn a_line_of_work_says_the_latest_thought_and_the_live_row_only_that_it_thinks() {
    let thinking = done("a", 1, reasoning("First\nThen this\n\n"));
    assert_eq!(work_line(&thinking, "/work"), "Then this");
    assert_eq!(live_line(Some(&thinking), "/work"), "Thinking…");
    assert_eq!(live_line(None, "/work"), "Working…");
    let agent = done("b", 1, ItemBody::Subagent(subagent("Check the callers")));
    assert_eq!(work_line(&agent, "/work"), "Check the callers");
    assert_eq!(live_line(Some(&agent), "/work"), "Check the callers");
    let running = Item {
        status: ItemStatus::Running,
        ..done("c", 1, ItemBody::Tool(tool_of(ToolFamily::Command, "ls")))
    };
    assert_eq!(live_line(Some(&running), "/work"), "Running ls");
}

#[test]
fn times_and_tokens_are_said_as_a_person_would() {
    assert_eq!(duration(41), "41s");
    assert_eq!(duration(63), "1m 3s");
    assert_eq!(duration(7500), "2h 5m");
    let compaction = |before, after| Compaction {
        trigger: CompactionTrigger::Auto,
        before,
        after,
    };
    assert_eq!(
        compacted(&compaction(Some(182_400), Some(999))),
        "Context compacted · 182k → 999 tokens"
    );
    assert_eq!(compacted(&compaction(None, Some(999))), "Context compacted");
    let mut settled = run("r1", RunStatus::Completed, 5);
    settled.model = Some("claude-opus".into());
    settled.effort = Some(Effort::Xhigh);
    assert_eq!(answer_meta(&settled), "claude-opus · Extra high");
}
