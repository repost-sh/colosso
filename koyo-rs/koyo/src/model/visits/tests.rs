//! The tabs to switch among: those looked at, every kind stamped as it shows, the latest first;
//! then the open ones not looked at yet, the nearest first; at most as many as asked for, each
//! where it is now, kept as it moves and gone once it's closed.

use crate::model::tests::{loose, page, with_pages};
use crate::model::{Pinned, Spot, Unloaded, Workspace};

/// The titles of the `most` tabs to switch among, pinned and loose pages too.
fn latest(workspace: &Workspace, most: usize) -> Vec<String> {
    (workspace.latest(most).iter())
        .map(|visited| visited.tab.first().title.to_string())
        .collect()
}

/// Where the tabs to switch among are.
fn spots(workspace: &Workspace) -> Vec<Spot> {
    (workspace.latest(5).iter())
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
    let mut workspace = with_pages(&["a", "b", "c"]);
    workspace.stamp_shown();
    for at in [1, 0, 2] {
        look_at(&mut workspace, at);
    }
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a", "c"]);
    workspace.stamp_shown();
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a", "c"]);
}

#[test]
fn open_tabs_not_looked_at_fill_the_list_the_nearest_first() {
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    let feature = workspace.unit_mut(0, 1).expect("the other worktree");
    feature.tabs.push(page("feature"));
    workspace.show_pages_of(7);
    workspace.bring_back_loose(7, vec![loose("here"), loose("beside")], None);
    workspace.bring_back_loose(9, vec![loose("there")], None);
    workspace.add_pinned(pinned("tile"), usize::MAX, false);
    let nearest = ["b", "New chat", "a", "here", "beside", "feature", "there"];
    assert_eq!(latest(&workspace, 10), nearest);
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
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    workspace.add_pinned(pinned("pinned"), usize::MAX, true);
    workspace.stamp_shown();
    workspace.show_pages_of(7);
    workspace.add_loose(loose("loose"), 0, true);
    workspace.stamp_shown();
    workspace.select_branch(0, 1);
    workspace.push_tab(page("feature"));
    workspace.stamp_shown();
    let everywhere = [
        Spot::Checkout(0, 1, 0),
        Spot::Loose(7),
        Spot::Pinned,
        Spot::Checkout(0, 0, 2),
        Spot::Checkout(0, 0, 0),
    ];
    assert_eq!(spots(&workspace), everywhere);
}

#[test]
fn a_tab_keeps_its_place_as_it_moves_and_leaves_once_closed() {
    let mut workspace = with_pages(&["a", "b"]);
    workspace.stamp_shown();
    look_at(&mut workspace, 1);
    look_at(&mut workspace, 0);
    let b = workspace.tab((0, 0, 2)).expect("b").first().id;
    assert_eq!(workspace.pin_tab((0, 0, 2), "b".into(), usize::MAX), Some(0));
    assert_eq!(latest(&workspace, 5), ["New chat", "a", "b"]);
    assert_eq!(workspace.spot_of(b), Some(Spot::Pinned));
    let closed = Unloaded {
        address: "b".into(),
        favicon: None,
        restored: true,
        session: koyo_browser_ipc::Session::default(),
    };
    assert!(workspace.close_pinned_page(0, "b".into(), closed));
    assert_eq!(latest(&workspace, 5), ["New chat", "a"]);
    assert!(workspace.show_pinned(0));
    workspace.stamp_shown();
    assert_eq!(latest(&workspace, 5), ["b", "New chat", "a"]);
    assert!(workspace.close_tab(0, 0, 1).is_some());
    assert_eq!(latest(&workspace, 5), ["b", "New chat"]);
    assert_eq!(workspace.spot_of(b), Some(Spot::Pinned));
}

#[test]
fn a_pane_is_found_shown_and_taken_out_the_same_way_wherever_its_tab_is() {
    let mut workspace = with_pages(&["a", "b"]);
    workspace.show_pages_of(7);
    workspace.bring_back_loose(7, vec![loose("here")], None);
    workspace.add_pinned(pinned("tile"), usize::MAX, false);
    let key = |workspace: &Workspace, title: &str| {
        (workspace.everywhere())
            .find(|visited| visited.tab.first().title.as_ref() == title)
            .map(|visited| visited.tab.first().id)
            .expect("the tab is open")
    };
    let mut beside = Vec::new();
    for title in ["a", "here", "tile"] {
        let id = key(&workspace, title);
        let tools = page("tools");
        beside.push(tools.first().id);
        let tab = workspace.tab_holding_mut(id).expect("its tab");
        assert!(tab.place(id, crate::model::Edge::Right, tools));
    }
    let a = key(&workspace, "a");
    let (here, tile) = (key(&workspace, "here"), key(&workspace, "tile"));
    let located = workspace.locate(|pane| pane.id == beside[0]).expect("beside a");
    assert_eq!((located.spot, located.tab), (Spot::Checkout(0, 0, 1), a));
    assert_eq!(workspace.spot_of(beside[1]), Some(Spot::Loose(7)));
    assert_eq!(workspace.spot_of(beside[2]), Some(Spot::Pinned));
    for (spot, key, title) in [(Spot::Pinned, tile, "tile"), (Spot::Loose(7), here, "here")] {
        assert!(workspace.show_tab(spot, key));
        let shown = workspace.tab_shown().expect("a tab shown");
        assert_eq!(shown.first().title.as_ref(), title);
    }
    assert!(workspace.show_tab(Spot::Checkout(0, 0, 2), key(&workspace, "b")));
    assert_eq!(workspace.tab_shown().map(|tab| tab.first().title.to_string()), Some("b".into()));
    assert!(!workspace.show_tab(Spot::Checkout(0, 0, 9), 0), "no such tab");
    for id in beside {
        assert!(workspace.remove_pane(id));
        assert_eq!(workspace.spot_of(id), None);
    }
    for id in [here, tile] {
        assert!(!workspace.remove_pane(id));
        assert!(workspace.pane(id).is_some());
    }
    assert!(workspace.remove_pane(a));
    assert_eq!(workspace.spot_of(a), None);
}
