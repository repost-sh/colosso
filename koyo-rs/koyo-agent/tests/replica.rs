//! A thread as the host last said it: each thing the socket can bring, merged into what's held.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails loudly at the step that broke"
)]

#[path = "support/records.rs"]
mod records;

use koyo_agent::protocol::events::{Appended, Changed};
use koyo_agent::protocol::ids::{ItemId, ThreadId};
use koyo_agent::protocol::methods::Rows;
use koyo_agent::protocol::model::{Item, Run, RunStatus};
use koyo_agent::replica::Replica;
use records::{THREAD, answer, item, message, run, snapshot, thread};

fn changed(seq: u64, runs: Vec<Run>, items: Vec<Item>) -> Changed {
    Changed {
        thread_id: ThreadId(THREAD.into()),
        seq,
        thread: None,
        runs,
        items,
        requests: Vec::new(),
    }
}

fn appended(item: &str, text: &str) -> Appended {
    Appended {
        thread_id: ThreadId(THREAD.into()),
        item_id: ItemId(item.into()),
        text: text.into(),
    }
}

fn text_of(replica: &Replica, id: &str) -> String {
    let item = (replica.items.iter()).find(|item| item.id.0 == id).unwrap();
    item.body.text().unwrap().to_owned()
}

fn ordinals(replica: &Replica) -> Vec<u64> {
    replica.items.iter().map(|item| item.ordinal).collect()
}

#[test]
fn a_change_adds_new_rows_in_their_order_and_replaces_the_ones_it_has_whole() {
    let mut replica = Replica::new(snapshot(
        3,
        vec![run("r1", RunStatus::Running, 3)],
        vec![item("a", 0, message("Why?"), 2), item("c", 2, answer("Be"), 3)],
    ));
    replica.apply(changed(
        4,
        vec![run("r1", RunStatus::Completed, 4)],
        vec![item("b", 1, answer("Think"), 4), item("c", 2, answer("Because."), 4)],
    ));
    assert_eq!(replica.seq(), 4);
    assert_eq!(ordinals(&replica), [0, 1, 2]);
    assert_eq!(text_of(&replica, "c"), "Because.");
    assert_eq!(replica.runs.len(), 1);
    assert_eq!(replica.runs[0].status, RunStatus::Completed);
    // The thread is told only when the step changed it.
    assert_eq!(replica.thread.title, "At 3");
    let mut titled = changed(5, Vec::new(), Vec::new());
    titled.thread = Some(thread(5));
    replica.apply(titled);
    assert_eq!(replica.thread.title, "At 5");
}

#[test]
fn text_grows_by_what_is_appended_and_a_whole_row_settles_it() {
    let mut replica = Replica::new(snapshot(1, Vec::new(), vec![item("a", 0, answer(""), 1)]));
    assert!(replica.append(&appended("a", "Bec")));
    assert!(replica.append(&appended("a", "ause")));
    assert_eq!(text_of(&replica, "a"), "Because");
    replica.apply(changed(2, Vec::new(), vec![item("a", 0, answer("Because."), 2)]));
    assert_eq!(text_of(&replica, "a"), "Because.");
}

#[test]
fn an_append_to_what_it_does_not_hold_or_to_what_has_no_text_changes_nothing() {
    let mut replica = Replica::new(snapshot(1, Vec::new(), vec![item("a", 0, message("Why?"), 1)]));
    let before = replica.clone();
    assert!(!replica.append(&appended("b", "lost")));
    assert!(!replica.append(&appended("a", "lost")));
    assert_eq!(replica, before);
}

#[test]
fn a_row_older_than_the_one_held_is_not_taken_though_one_of_the_same_change_is() {
    let mut replica = Replica::new(snapshot(5, Vec::new(), vec![item("a", 0, answer("New"), 5)]));
    replica.apply(changed(4, Vec::new(), vec![item("a", 0, answer("Old"), 4)]));
    assert_eq!(text_of(&replica, "a"), "New");
    assert_eq!(replica.seq(), 5);
    // A streaming item's text grows without a change number, and a reopen sends it whole.
    let mut grown = snapshot(5, Vec::new(), vec![item("a", 0, answer("New and more"), 5)]);
    grown.thread.title = "Reopened".into();
    replica.catch_up(grown);
    assert_eq!(text_of(&replica, "a"), "New and more");
    assert_eq!(replica.thread.title, "Reopened");
}

#[test]
fn a_reopen_merges_what_changed_while_away_and_keeps_the_rest() {
    let mut replica = Replica::new(snapshot(
        2,
        vec![run("r1", RunStatus::Running, 2)],
        vec![item("a", 0, message("Why?"), 1), item("b", 1, answer("Part"), 2)],
    ));
    replica.catch_up(snapshot(
        9,
        vec![run("r1", RunStatus::Completed, 9)],
        vec![item("b", 1, answer("Part and whole"), 9), item("c", 2, answer("More"), 8)],
    ));
    assert_eq!(replica.seq(), 9);
    assert_eq!(ordinals(&replica), [0, 1, 2]);
    assert_eq!(text_of(&replica, "b"), "Part and whole");
    assert_eq!(replica.runs[0].status, RunStatus::Completed);
}

#[test]
fn history_goes_before_what_is_held_and_leaves_the_change_number_alone() {
    let mut replica = Replica::new(snapshot(
        9,
        vec![run("r2", RunStatus::Completed, 9)],
        vec![item("c", 4, answer("Latest"), 9)],
    ));
    replica.prepend(Rows {
        runs: vec![run("r1", RunStatus::Completed, 3)],
        items: vec![item("a", 0, message("Why?"), 1), item("b", 3, answer("Old"), 3)],
        requests: Vec::new(),
    });
    assert_eq!(replica.seq(), 9);
    assert_eq!(ordinals(&replica), [0, 3, 4]);
    let runs: Vec<_> = replica.runs.iter().map(|run| run.id.0.as_str()).collect();
    assert_eq!(runs, ["r1", "r2"]);
    // A page entirely older than what's held goes before it, in its order.
    replica.prepend(Rows {
        runs: vec![run("r0", RunStatus::Completed, 1)],
        items: Vec::new(),
        requests: Vec::new(),
    });
    let runs: Vec<_> = replica.runs.iter().map(|run| run.id.0.as_str()).collect();
    assert_eq!(runs, ["r0", "r1", "r2"]);
}
