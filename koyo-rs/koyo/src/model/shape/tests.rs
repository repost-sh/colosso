use koyo_agent::protocol::ids::ThreadId;

use crate::model::tests::{chat, workspace};
use crate::model::{ClosedFrom, ClosedTab, Pane, PaneKind, SplitAxis, Tab, Workspace};

fn give_the_first_pane(workspace: &mut Workspace, give: impl FnOnce(&mut Pane)) {
    let tab = workspace.tab_shown_mut().unwrap();
    give(tab.panes_mut().remove(0));
}

#[test]
fn a_tab_opening_or_closing_changes_the_tabs_and_leaves_the_checkouts_alone() {
    let mut workspace = workspace(&[]);
    let (checkouts, tabs) = (workspace.checkouts_shape(), workspace.tabs_shape());

    workspace.new_tab(PaneKind::Chat);
    assert_eq!(workspace.checkouts_shape(), checkouts);
    assert_ne!(workspace.tabs_shape(), tabs);

    workspace.close_tab(0, 0, 0);
    assert_eq!(workspace.checkouts_shape(), checkouts);
    assert_eq!(workspace.tabs_shape(), tabs);
}

#[test]
fn a_checkout_coming_changes_the_checkouts() {
    let before = workspace(&[]).checkouts_shape();
    assert_eq!(workspace(&[]).checkouts_shape(), before);
    assert_ne!(workspace(&["feature"]).checkouts_shape(), before);
}

#[test]
fn two_tabs_merged_into_a_split_are_not_the_shape_of_the_two() {
    let (first, second) = (chat("first"), chat("second"));
    let (mut apart, mut merged) = (workspace(&[]), workspace(&[]));
    apart.push_tab(first.clone());
    apart.push_tab(second.clone());
    merged.push_tab(Tab::split(SplitAxis::Row, first, second));
    assert_ne!(apart.tabs_shape(), merged.tabs_shape());
}

#[test]
fn a_pane_given_a_terminal_or_a_chat_thread_changes_the_tabs() {
    let mut workspace = workspace(&[]);
    workspace.new_tab(PaneKind::Chat);
    let before = workspace.tabs_shape();

    give_the_first_pane(&mut workspace, |pane| pane.chat = Some(ThreadId("thread".into())));
    let with_thread = workspace.tabs_shape();
    assert_ne!(with_thread, before);

    give_the_first_pane(&mut workspace, |pane| pane.terminal = Some(7));
    assert_ne!(workspace.tabs_shape(), with_thread);
}

#[test]
fn a_closed_tab_is_still_among_the_tabs() {
    let mut workspace = workspace(&[]);
    let before = workspace.tabs_shape();
    workspace.new_tab(PaneKind::Chat);
    let tab = workspace.close_tab(0, 0, 0).unwrap();
    workspace.remember_closed(ClosedTab {
        from: ClosedFrom::Checkout("/src/app".into()),
        at: 0,
        tab,
        addresses: Vec::new(),
        files: Vec::new(),
        changes: Vec::new(),
        devices: Vec::new(),
    });
    assert_ne!(workspace.tabs_shape(), before);
}
