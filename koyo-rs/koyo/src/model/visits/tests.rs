//! The tabs to switch among: those looked at, every kind stamped as it shows, the latest first;
//! then the open ones not looked at yet, the nearest first; at most as many as asked for, each
//! where it is now, kept as it moves and gone once it's closed.

use crate::model::tests::{loose, page, with_pages};
use crate::model::{Pinned, Spot, Unloaded, Workspace};

/// The titles of the `most` tabs to switch among, pinned and loose pages too.
fn latest(workspace: &Workspace, most: usize) -> Vec<String> {
    (workspace.latest(most, true).iter())
        .map(|visited| visited.tab.first().title.to_string())
        .collect()
}

/// Where the tabs to switch among are; pinned and loose pages too if `apart`.
fn spots(workspace: &Workspace, apart: bool) -> Vec<Spot> {
    (workspace.latest(5, apart).iter())
        .map(|visited| visited.spot)
        .collect()
}

/// Shows the main checkout's tab at `at`, stamped as the shell stamps each change.
fn look_at(workspace: &mut Workspace, at: usize) {
    workspace.select_tab(0, 0, at);
    workspace.stamp_shown();
}

/// A page pinned from the project.
fn pinned(address: &str) -> Pinned {
    Pinned {
        tab: page(address),
        home: address.into(),
        project: "/src/app".into(),
        more: serde_json::Map::new(),
    }
}

#[test]
fn the_tabs_looked_at_are_listed_the_latest_first() {
    // A chat and the pages a, b and c, c shown.
    let mut workspace = with_pages(&["a", "b", "c"]);
    workspace.stamp_shown();
    for at in [1, 0, 2] {
        look_at(&mut workspace, at);
    }
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a", "c"]);
    // Stamped again with nothing changed, the one shown stays where it is.
    workspace.stamp_shown();
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a", "c"]);
}

#[test]
fn open_tabs_not_looked_at_fill_the_list_the_nearest_first() {
    // The main checkout's chat and pages a and b, b shown and the only tab looked at; a tab on
    // the other worktree; two loose pages in the space shown and one in another; a pinned page.
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    let feature = workspace.unit_mut(0, 1).expect("the other worktree");
    feature.tabs.push(page("feature"));
    workspace.show_pages_of(7);
    workspace.bring_back_loose(7, vec![loose("here"), loose("beside")], None);
    workspace.bring_back_loose(9, vec![loose("there")], None);
    workspace.add_pinned(pinned("tile"), false);
    // After b: its checkout's tabs, then what else the sidebar lists, the space's loose pages
    // and the project's other worktree, then another space's. The pinned page, a tile with no
    // page open, isn't among them.
    let nearest = ["b", "New chat", "a", "here", "beside", "feature", "there"];
    assert_eq!(latest(&workspace, 10), nearest);
    // A loose page shown: the space's other loose page is the one beside it.
    assert!(workspace.show_loose(0));
    workspace.stamp_shown();
    let from_loose = ["here", "b", "beside", "New chat", "a", "feature", "there"];
    assert_eq!(latest(&workspace, 10), from_loose);
}

#[test]
fn only_as_many_as_asked_for_are_listed_the_one_shown_first() {
    let mut workspace = with_pages(&["a", "b", "c", "d", "e"]);
    workspace.stamp_shown();
    for at in 0..5 {
        look_at(&mut workspace, at);
    }
    assert_eq!(latest(&workspace, 5), ["d", "c", "b", "a", "New chat"]);
    assert_eq!(latest(&workspace, 2), ["d", "c"]);
}

#[test]
fn pinned_and_loose_pages_and_other_checkouts_are_looked_at_as_any_tab() {
    // The main checkout's chat and pages a and b, b shown.
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    workspace.add_pinned(pinned("pinned"), true);
    workspace.stamp_shown();
    // A loose page of the space known by 7, shown there.
    workspace.show_pages_of(7);
    workspace.add_loose(loose("loose"), 0, true);
    workspace.stamp_shown();
    // A tab of the other worktree.
    workspace.select_branch(0, 1);
    workspace.push_tab(page("feature"));
    workspace.stamp_shown();
    // The four looked at, the feature's tab, the loose page, the pinned one and b, then the
    // main checkout's chat, not looked at, in the room left.
    let everywhere = [
        Spot::Checkout(0, 1, 0),
        Spot::Loose(7),
        Spot::Pinned,
        Spot::Checkout(0, 0, 2),
        Spot::Checkout(0, 0, 0),
    ];
    assert_eq!(spots(&workspace, true), everywhere);
    // The Orca layout lists no pinned or loose page.
    let checkouts = [
        Spot::Checkout(0, 1, 0),
        Spot::Checkout(0, 0, 2),
        Spot::Checkout(0, 0, 0),
        Spot::Checkout(0, 0, 1),
    ];
    assert_eq!(spots(&workspace, false), checkouts);
}

#[test]
fn a_tab_keeps_its_place_as_it_moves_and_leaves_once_closed() {
    // The chat, a and b, b shown; then a, then the chat.
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    look_at(&mut workspace, 1);
    look_at(&mut workspace, 0);
    // b pinned keeps its place, found where it is now.
    let b = workspace.tab((0, 0, 2)).expect("b").first().id;
    assert_eq!(workspace.pin_tab((0, 0, 2), "b".into()), Some(0));
    assert_eq!(latest(&workspace, 5), ["New chat", "a", "b"]);
    assert_eq!(workspace.spot_of(b), Some(Spot::Pinned));
    // Its page closed, it stays a tile but leaves the list.
    let closed = Unloaded {
        address: "b".into(),
        favicon: None,
    };
    assert!(workspace.close_pinned_page(0, "b".into(), closed));
    assert_eq!(latest(&workspace, 5), ["New chat", "a"]);
    // Picked again, it's back, the latest.
    assert!(workspace.show_pinned(0));
    workspace.stamp_shown();
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a"]);
    // A tab closed leaves it.
    assert!(workspace.close_tab(0, 0, 1).is_some());
    assert_eq!(latest(&workspace, 5), ["b", "New chat"]);
    assert_eq!(workspace.spot_of(b), Some(Spot::Pinned));
}
