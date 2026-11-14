//! The loose pages on the workspace: each space's own, the one a space shows, and a page's tab
//! moving between a checkout's tabs, the loose pages and the pinned ones.

use std::path::Path;

use crate::model::Workspace;
use crate::model::tests::{loose, shown};

/// The workspace of the pinned pages' tests, the space known by 1 shown.
fn with_pages(addresses: &[&str]) -> Workspace {
    let mut workspace = crate::model::tests::with_pages(addresses);
    workspace.show_pages_of(1);
    workspace
}

/// The loose pages of the space shown, by their titles.
fn listed(workspace: &Workspace) -> Vec<String> {
    let titles = workspace
        .loose()
        .iter()
        .map(|page| page.tab.first().title.to_string());
    titles.collect()
}

#[test]
fn each_space_has_its_own_loose_pages_and_shows_the_one_it_showed_last() {
    let mut workspace = with_pages(&[]);
    assert_eq!(workspace.add_loose(loose("a"), usize::MAX, true), 0);
    assert_eq!(workspace.add_loose(loose("b"), usize::MAX, false), 1);
    assert_eq!(
        (listed(&workspace), workspace.loose_shown()),
        (vec!["a".into(), "b".into()], Some(0))
    );
    assert_eq!(shown(&workspace), "a", "in place of the checkout's tab");

    // Another space lists none of them, and shows the checkout's tab.
    assert!(workspace.show_pages_of(2));
    assert!(!workspace.show_pages_of(2), "shown already");
    assert!(listed(&workspace).is_empty());
    assert_eq!(
        (workspace.loose_shown(), shown(&workspace).as_str()),
        (None, "New chat")
    );
    workspace.add_loose(loose("c"), 0, true);
    assert_eq!(shown(&workspace), "c");
    // Back in the first, its own, the one it showed shown.
    workspace.show_pages_of(1);
    assert_eq!(
        (listed(&workspace), shown(&workspace)),
        (vec!["a".into(), "b".into()], "a".into())
    );
    let by_space: Vec<_> = (workspace.loose_by_space())
        .map(|(space, pages, shown)| (space, pages.len(), shown))
        .collect();
    assert_eq!(by_space, [(1, 2, Some(0)), (2, 1, Some(0))]);

    // A tab of a checkout picked leaves it, in this space only.
    workspace.select_tab(0, 0, 0);
    assert_eq!(
        (workspace.loose_shown(), shown(&workspace).as_str()),
        (None, "New chat")
    );
    workspace.show_pages_of(2);
    assert_eq!(shown(&workspace), "c");
}

#[test]
fn a_loose_page_moves_among_them_and_closed_leaves_its_neighbour_shown() {
    let mut workspace = with_pages(&[]);
    for address in ["a", "b", "c"] {
        workspace.add_loose(loose(address), usize::MAX, false);
    }
    assert!(workspace.show_loose(1));
    assert!(!workspace.show_loose(7), "no such page");
    assert!(workspace.move_loose(0, 2));
    assert_eq!(listed(&workspace), ["b", "c", "a"]);
    assert_eq!(shown(&workspace), "b", "the same page, moved");
    assert!(!workspace.move_loose(5, 0), "no such place");
    let key = workspace.loose()[2].key();
    assert_eq!(workspace.loose_at(key), Some(2));

    // One that isn't shown closes: the one shown stays.
    let closed = workspace.close_loose(2).expect("a page");
    assert_eq!(closed.tab.first().title.as_ref(), "a");
    assert_eq!(shown(&workspace), "b");
    // The first, shown, closes: the next shows. The last closes: the checkout's tab shows.
    assert!(workspace.close_loose(0).is_some());
    assert_eq!(
        (listed(&workspace), shown(&workspace)),
        (vec!["c".to_owned()], "c".to_owned())
    );
    assert!(workspace.close_loose(0).is_some());
    assert!(workspace.close_loose(0).is_none());
    assert_eq!(
        (workspace.loose_shown(), shown(&workspace).as_str()),
        (None, "New chat")
    );
    // Of two, the second shown closes: the one before it shows.
    workspace.add_loose(loose("d"), usize::MAX, false);
    workspace.add_loose(loose("e"), usize::MAX, true);
    workspace.close_loose(1);
    assert_eq!(shown(&workspace), "d");
}

#[test]
fn a_pages_tab_moves_between_a_checkout_the_loose_pages_and_the_pinned() {
    let mut workspace = with_pages(&["a", "b"]);
    // A chat isn't taken; a page is, and shows among the loose pages, of its project's profile.
    assert!(!workspace.loosen_tab((0, 0, 0), 0), "a chat stays a tab");
    assert!(workspace.loosen_tab((0, 0, 1), 0));
    assert_eq!(
        (listed(&workspace), shown(&workspace)),
        (vec!["a".to_owned()], "a".to_owned())
    );
    assert_eq!(workspace.loose()[0].project, Path::new("/src/app"));
    assert_eq!(workspace.branch().expect("a checkout").tabs.len(), 2);
    assert!(workspace.loosen_tab((0, 0, 1), 0), "before the first");
    assert_eq!(listed(&workspace), ["b", "a"]);
    assert_eq!(workspace.shown_apart_project(), Some(Path::new("/src/app")));

    // The loose page shown, pinned, goes on showing; a pinned page shown is over the loose ones.
    assert_eq!(workspace.pin_loose(0, "b".into()), Some(0));
    assert_eq!(
        (listed(&workspace), workspace.loose_shown()),
        (vec!["a".to_owned()], None)
    );
    assert_eq!(
        (workspace.pinned_shown(), shown(&workspace).as_str()),
        (Some(0), "b")
    );
    assert!(workspace.apart_shown());
    // Unpinned among the loose pages, first, it shows there.
    assert!(workspace.loosen_pinned(0, 0));
    assert!(!workspace.loosen_pinned(0, 0), "none pinned now");
    assert_eq!(
        (listed(&workspace), workspace.loose_shown()),
        (vec!["b".into(), "a".into()], Some(0))
    );

    // Onto the worktree's tabs: a tab of that checkout, shown, and the loose page left for it.
    assert!(!workspace.settle_loose(0, (0, 9), 0), "no such checkout");
    assert!(!workspace.settle_loose(9, (0, 1), 0), "no such page");
    assert!(workspace.settle_loose(1, (0, 1), usize::MAX));
    assert_eq!(listed(&workspace), ["b"]);
    assert!(!workspace.apart_shown());
    assert_eq!(workspace.project().expect("a project").active_branch, 1);
    assert_eq!(shown(&workspace), "a");
    // Every pane is found wherever its page went.
    let key = workspace.loose()[0].key();
    assert_eq!(
        workspace
            .pane(key)
            .map(|pane| pane.title.to_string())
            .as_deref(),
        Some("b")
    );
    let apart = workspace.find_apart(|pane| pane.id == key);
    assert_eq!(apart, Some((key, Path::new("/src/app"))));
    assert!(workspace.pane_mut(key).is_some() && workspace.tab_holding_mut(key).is_some());
}

#[test]
fn a_deleted_spaces_loose_pages_go_where_its_projects_do() {
    let mut workspace = with_pages(&[]);
    workspace.add_loose(loose("a"), usize::MAX, true);
    workspace.show_pages_of(2);
    workspace.add_loose(loose("b"), usize::MAX, true);
    // The second space goes, its projects to the first: so do its pages, after the first's.
    assert!(workspace.hand_on_loose(2, Some(1)).is_empty());
    workspace.show_pages_of(1);
    assert_eq!(
        (listed(&workspace), shown(&workspace)),
        (vec!["a".into(), "b".into()], "a".into())
    );
    // The first goes with its projects: its pages are handed back, to close.
    let closed = workspace.hand_on_loose(1, None);
    assert_eq!(closed.len(), 2);
    assert!(listed(&workspace).is_empty());
    assert!(
        workspace.hand_on_loose(7, None).is_empty(),
        "a space with none"
    );

    // Brought back with the window, after what the space has, the one it showed shown.
    workspace.add_loose(loose("c"), usize::MAX, false);
    workspace.bring_back_loose(1, vec![loose("d"), loose("e")], Some(1));
    assert_eq!(
        (listed(&workspace), shown(&workspace)),
        (vec!["c".into(), "d".into(), "e".into()], "e".into())
    );
}
