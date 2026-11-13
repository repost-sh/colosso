//! The pinned pages on the workspace: what a pin takes, the one shown, and pinning, moving,
//! closing, unpinning and removing.

use crate::model::tests::{shown, with_pages};
use crate::model::{Edge, PaneKind, Unloaded, Workspace};

/// Pins the first of the main checkout's pages, the tab after its chat, where it is.
fn pin_first_page(workspace: &mut Workspace) {
    let home = workspace
        .tab((0, 0, 1))
        .expect("a page")
        .first()
        .title
        .to_string();
    assert!(workspace.pin_tab((0, 0, 1), home).is_some());
}

fn pinned(workspace: &Workspace) -> Vec<String> {
    (workspace.pinned().iter())
        .map(|pinned| pinned.tab.first().title.to_string())
        .collect()
}

#[test]
fn only_a_tab_whose_single_pane_is_a_page_is_pinned() {
    let mut workspace = with_pages(&["a", "b"]);
    assert_eq!(
        workspace.pin_tab((0, 0, 0), String::new()),
        None,
        "a chat isn't"
    );
    // The two pages side by side: a split isn't.
    let unit = workspace.branch_mut().expect("a checkout");
    let beside = unit.tabs[1].first().id;
    assert!(unit.drop_tab(2, beside, Edge::Right));
    assert_eq!(
        workspace.pin_tab((0, 0, 1), String::new()),
        None,
        "a split isn't"
    );
    assert_eq!(
        workspace.pin_tab((0, 0, 9), String::new()),
        None,
        "no such tab"
    );
    assert!(workspace.pinned().is_empty());
    assert_eq!(workspace.branch().expect("a checkout").tabs.len(), 2);
}

#[test]
fn a_page_pinned_leaves_its_checkout_and_goes_on_showing_if_it_was_shown() {
    let mut workspace = with_pages(&["a", "b"]);
    // `a` isn't the tab shown: pinned, the checkout still shows `b`.
    assert_eq!(workspace.pin_tab((0, 0, 1), "a".into()), Some(0));
    assert_eq!(pinned(&workspace), ["a"]);
    assert_eq!(
        workspace.pinned()[0].project,
        std::path::Path::new("/src/app")
    );
    assert_eq!(workspace.pinned_shown(), None);
    assert_eq!(shown(&workspace), "b");
    // `b` is: pinned, it's still what's shown, and its checkout shows the chat under it.
    assert_eq!(workspace.pin_tab((0, 0, 1), "b".into()), Some(1));
    assert_eq!(pinned(&workspace), ["a", "b"]);
    assert_eq!(workspace.pinned_shown(), Some(1));
    assert_eq!(shown(&workspace), "b");
    let unit = workspace.branch().expect("a checkout");
    assert_eq!((unit.tabs.len(), unit.active_tab), (1, 0));
}

#[test]
fn a_pinned_page_stays_shown_while_another_project_is_selected_until_a_tab_is_picked() {
    let mut workspace = with_pages(&["a"]);
    pin_first_page(&mut workspace);
    assert!(workspace.show_pinned(0));
    assert!(!workspace.show_pinned(3), "no such pinned page");
    // Another space's project, or none, as an empty space shows.
    workspace.select_project(None);
    assert_eq!(shown(&workspace), "a", "with no project shown");
    workspace.select_project(Some(0));
    assert_eq!(shown(&workspace), "a");
    // A tab of a checkout picked, a worktree selected or a tab opened leaves it.
    workspace.select_tab(0, 0, 0);
    assert_eq!(
        (workspace.pinned_shown(), shown(&workspace).as_str()),
        (None, "New chat")
    );
    workspace.show_pinned(0);
    workspace.select_branch(0, 1);
    assert_eq!(workspace.pinned_shown(), None);
    assert!(workspace.tab_shown().is_none(), "the worktree has no tab");
    workspace.show_pinned(0);
    workspace.new_tab(PaneKind::Chat);
    assert_eq!(workspace.pinned_shown(), None);
    workspace.show_pinned(0);
    assert!(workspace.leave_apart());
    assert!(!workspace.leave_apart(), "none was shown");
    assert_eq!(pinned(&workspace), ["a"], "left, it's still pinned");
}

#[test]
fn pinned_pages_move_among_themselves_and_the_one_shown_stays_shown() {
    let mut workspace = with_pages(&["a", "b", "c"]);
    for _ in 0..3 {
        pin_first_page(&mut workspace);
    }
    workspace.show_pinned(1);
    assert!(workspace.move_pinned(0, 2));
    assert_eq!(pinned(&workspace), ["b", "c", "a"]);
    assert_eq!(shown(&workspace), "b", "the same page, moved");
    assert!(workspace.move_pinned(2, 0));
    assert_eq!(pinned(&workspace), ["a", "b", "c"]);
    assert_eq!(workspace.pinned_shown(), Some(1));
    assert!(!workspace.move_pinned(1, 1), "where it is");
    assert!(!workspace.move_pinned(0, 3), "no such slot");
}

#[test]
fn a_pinned_page_closed_stays_pinned_and_opens_where_it_was_once_picked_again() {
    let mut workspace = with_pages(&["a", "b"]);
    for _ in 0..2 {
        pin_first_page(&mut workspace);
    }
    workspace.show_pinned(1);
    let closed = |address: &str| Unloaded {
        address: address.into(),
        favicon: None,
    };
    // One that isn't shown closes: the page shown stays shown, and both stay pinned.
    assert!(workspace.close_pinned_page(0, "a".into(), closed("https://a.example/now")));
    assert_eq!(pinned(&workspace), ["a", "b"]);
    assert_eq!(shown(&workspace), "b");
    let page = workspace.pinned()[0].tab.first();
    assert_eq!(page.unloaded_address(), Some("https://a.example/now"));
    // The one shown closes: the checkout's tab shows in its place, and it's still pinned.
    assert!(workspace.close_pinned_page(1, "b".into(), closed("https://b.example/")));
    assert_eq!(
        (workspace.pinned_shown(), shown(&workspace).as_str()),
        (None, "New chat")
    );
    assert_eq!(pinned(&workspace), ["a", "b"]);
    assert!(
        !workspace.close_pinned_page(5, "".into(), closed("")),
        "no such pinned page"
    );
    // Its tile picked again, it's the tab shown, to open at the address it closed at.
    assert!(workspace.show_pinned(1));
    let page = workspace.tab_shown().expect("the pinned page").first();
    assert_eq!(page.unloaded_address(), Some("https://b.example/"));
    // The address it's pinned at is its own to change, and goes with it when it's removed.
    assert_eq!(workspace.pinned()[0].home, "a");
    assert!(workspace.set_pinned_home(0, "https://a.example/".into()));
    assert!(
        !workspace.set_pinned_home(0, "https://a.example/".into()),
        "there already"
    );
    assert!(
        !workspace.set_pinned_home(7, "x".into()),
        "no such pinned page"
    );
    let removed = workspace.remove_pinned(0).expect("a pinned page");
    assert_eq!(removed.home, "https://a.example/");
    assert_eq!(
        (pinned(&workspace), workspace.pinned_shown()),
        (vec!["b".to_owned()], Some(0))
    );
    assert!(workspace.remove_pinned(3).is_none());
}

#[test]
fn a_page_unpinned_is_a_tab_of_the_checkout_shown_there() {
    let mut workspace = with_pages(&["a", "b"]);
    for _ in 0..2 {
        pin_first_page(&mut workspace);
    }
    workspace.show_pinned(1);
    // One before the page shown is unpinned onto the worktree, after its tabs: it's shown
    // there, the worktree selected, and the pinned page is left for it.
    assert!(!workspace.unpin(0, (0, 7), 0), "no such checkout");
    assert!(!workspace.unpin(4, (0, 1), 0), "no such pinned page");
    assert!(workspace.unpin(0, (0, 1), usize::MAX));
    assert_eq!(pinned(&workspace), ["b"]);
    assert_eq!(workspace.pinned_shown(), None);
    let project = workspace.project().expect("a project");
    assert_eq!(project.active_branch, 1);
    assert_eq!(shown(&workspace), "a");
    // And the other, first among the main checkout's tabs.
    assert!(workspace.unpin(0, (0, 0), 0));
    assert!(workspace.pinned().is_empty());
    let unit = workspace.branch().expect("a checkout");
    assert_eq!((unit.active_tab, unit.tabs.len()), (0, 2));
    assert_eq!(shown(&workspace), "b");
}

#[test]
fn a_pane_is_found_among_the_pinned_as_on_a_checkout() {
    let mut workspace = with_pages(&["a"]);
    let chat = workspace.branch().expect("a checkout").tabs[0].first().id;
    let page = workspace.branch().expect("a checkout").tabs[1].first().id;
    pin_first_page(&mut workspace);
    let apart = |id| workspace.find_apart(|pane| pane.id == id);
    assert_eq!(apart(page), Some((page, std::path::Path::new("/src/app"))));
    assert_eq!(apart(chat), None);
    assert_eq!(workspace.pinned_at(page), Some(0));
    assert!(
        workspace.find_pane(|pane| pane.id == page).is_none(),
        "on no checkout"
    );
    let found = |id| workspace.pane(id).map(|pane| pane.title.to_string());
    assert_eq!(found(page).as_deref(), Some("a"));
    assert_eq!(found(chat).as_deref(), Some("New chat"));
    assert_eq!(found(page + 1000), None);
}
