//! The rows of a run while it runs: the live row, there from the start unless what the run does
//! shows for itself, and the work it did beside it, grouped as each step's done, under ids that
//! stay the same as the rows grow.

use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, RunStatus, ToolFamily};
use koyo_agent::protocol::requests::RequestStatus;
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{RowId, RowKind, WorkFold, rows};

use super::records::{answer, done, item, request, run, snapshot, tool_of};
use super::{asks, made, of, replica, replica_of, shape, tool};

#[test]
fn work_alone_waits_on_the_live_row_while_it_runs_and_leaves_the_work_beside_it_be() {
    let edit = ItemBody::Tool(tool_of(ToolFamily::Edit, "src/main.rs"));
    let running = Item {
        status: ItemStatus::Running,
        ..item("c", 2, edit, 2)
    };
    let items = vec![
        done("b", 1, tool("one")),
        running.clone(),
        done("d", 3, tool("two")),
    ];
    let working = replica(vec![run("r1", RunStatus::Running, 5)], items);
    let alone = |item: &Item| item.id.0 == "c";
    let shown = rows(&working, |_| WorkFold::None, alone);
    assert_eq!(shape(&working, &shown), ["work b,d", "live"]);

    let finished = Item {
        status: ItemStatus::Completed,
        seq: 6,
        ..running
    };
    let items = vec![
        done("b", 1, tool("one")),
        finished,
        done("d", 3, tool("two")),
    ];
    let settled = replica(vec![run("r1", RunStatus::Running, 5)], items);
    let shown = rows(&settled, |_| WorkFold::None, alone);
    assert_eq!(
        shape(&settled, &shown),
        ["work b", "work c", "work d", "live"]
    );
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
