//! Which rows a thread's records make: each rule of the timeline, with no drawing.

#[expect(dead_code, reason = "each test crate builds what it needs of these")]
#[path = "support/records.rs"]
mod records;

use std::collections::HashSet;

use koyo_agent::protocol::ids::{ItemId, RequestId, RunId};
use koyo_agent::protocol::model::{
    Compaction, CompactionTrigger, FileChange, Item, ItemBody, ItemStatus, Notice, NoticeLevel,
    RequestRef, Run, RunStatus, ToolFamily,
};
use koyo_agent::protocol::requests::{Request, RequestStatus};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind, WorkFold, changed, rows};
use records::{answer, done, item, message, request, run, snapshot, subagent, tool_of};

fn tool(label: &str) -> ItemBody {
    ItemBody::Tool(tool_of(ToolFamily::Read, label))
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
            RowKind::Subagents { items } => {
                format!("agents {}", items.iter().map(id).collect::<Vec<_>>().join(" "))
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
    rows(replica, |_| WorkFold::Closed)
}

fn of(replica: &Replica) -> Vec<String> {
    shape(replica, &made(replica))
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
fn a_settled_run_folds_only_its_work() {
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
        ["user a", "fold", "text c", "request d", "compaction e", "answer g"]
    );

    let open = HashSet::from([RunId("r1".into())]);
    let unfolded = |run: &RunId| if open.contains(run) { WorkFold::Open } else { WorkFold::Closed };
    let all = [
        "user a",
        "fold",
        "work b",
        "text c",
        "request d",
        "compaction e",
        "work f",
        "answer g",
    ];
    assert_eq!(shape(&replica, &rows(&replica, unfolded)), all);

    let in_the_open = rows(&replica, |_| WorkFold::None);
    let without_a_fold: Vec<_> = all.into_iter().filter(|row| *row != "fold").collect();
    assert_eq!(shape(&replica, &in_the_open), without_a_fold);
}

#[test]
fn what_was_said_never_folds_however_much_came_after_it() {
    let essay = |tool_after| {
        let mut items = vec![done("a", 0, message("An essay, please")), done("b", 1, answer("An essay"))];
        items.extend(tool_after);
        items.push(done("d", 3, answer("Anything else?")));
        replica_of(RunStatus::Completed, items)
    };
    assert_eq!(
        of(&essay(None)),
        ["user a", "text b", "answer d"],
        "with no work there's nothing to open"
    );
    assert_eq!(
        of(&essay(Some(done("c", 2, tool("one"))))),
        ["user a", "text b", "fold", "answer d"]
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
fn subagents_one_after_the_other_are_a_row_that_stays_when_the_run_folds() {
    let agent = |id: &str, ordinal| done(id, ordinal, ItemBody::Subagent(Box::new(subagent("Check"))));
    let items = vec![
        done("a", 0, tool("first")),
        agent("b", 1),
        agent("c", 2),
        done("d", 3, tool("later")),
        agent("e", 4),
        done("f", 5, answer("Done")),
    ];
    let replica = replica_of(RunStatus::Completed, items);
    let open = rows(&replica, |_| WorkFold::Open);
    assert_eq!(shape(&replica, &open)[1..], [
        "work a",
        "agents b c",
        "work d",
        "agents e",
        "answer f",
    ]);
    assert_eq!(of(&replica), ["fold", "agents b c e", "answer f"]);
    let folded: Vec<_> = made(&replica).into_iter().map(|row| row.id).collect();
    let id = |item: &str| ItemId(item.into());
    assert_eq!(folded, [
        RowId::Fold(RunId("r1".into())),
        RowId::Subagents(id("b")),
        RowId::Item(id("f")),
    ]);
}

#[test]
fn a_running_subagent_shows_for_itself_and_the_live_row_says_the_rest() {
    let running = Item {
        status: ItemStatus::Running,
        ..done("a", 0, ItemBody::Subagent(Box::new(subagent("Check"))))
    };
    let replica = replica_of(RunStatus::Running, vec![running]);
    let kinds: Vec<_> = rows(&replica, |_| WorkFold::None).into_iter().map(|row| row.kind).collect();
    assert_eq!(kinds, [
        RowKind::Subagents { items: vec![0] },
        RowKind::Live { item: None },
    ]);
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
fn the_oldest_request_waiting_is_the_one_asked_and_the_next_is_once_it_is_answered() {
    let items = vec![done("b", 1, asks("q2")), done("c", 2, asks("q1"))];
    let waiting = |id: &str, seq: u64| Request {
        seq,
        ..request(id, RequestStatus::Pending)
    };
    let asked = |replica: &Replica| -> Vec<bool> {
        made(replica)
            .iter()
            .filter_map(|row| match row.kind {
                RowKind::Request { asking, .. } => Some(asking),
                _ => None,
            })
            .collect()
    };
    let mut snap = snapshot(9, vec![run("r1", RunStatus::Running, 5)], items);
    snap.requests = vec![waiting("q1", 3), waiting("q2", 4)];
    let mut replica = Replica::new(snap);
    assert_eq!(replica.asking().map(|asked| asked.id.0.as_str()), Some("q1"));
    assert_eq!(asked(&replica), [false, true]);
    let before = made(&replica);

    replica.requests.remove(0);
    replica.requests.insert(0, request("q1", RequestStatus::Answered));

    assert_eq!(replica.asking().map(|asked| asked.id.0.as_str()), Some("q2"));
    assert_eq!(asked(&replica), [true, false]);
    let after = made(&replica);
    assert!(changed(&before, &after).is_some(), "the row now asked is drawn anew");
}
