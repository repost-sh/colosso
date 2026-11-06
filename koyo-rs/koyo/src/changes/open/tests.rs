//! The changes' diffs in a workspace tab: a file picked in the list opens it once, right after
//! the tab shown, and shows it again from wherever; closed, ⌘⇧T brings it back.

use std::time::{Duration, Instant};

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_project::Feed;

use super::super::ChangesPanel;
use crate::checkout::Checkout;
use crate::files::tests::{tabs, with_shell};
use crate::right_panel::ToggleChanges;
use crate::shell::Shell;
use crate::test_support::{repo, select, shell_with, write};

/// Lets the checkout behind `panel` read until it's `done`, waiting for its project
/// between looks.
fn settle(panel: &Entity<ChangesPanel>, cx: &mut TestAppContext, done: impl Fn(&Checkout) -> bool) {
    cx.run_until_parked();
    let checkout = cx.read(|cx| panel.read(cx).checkout().clone());
    let project = cx.read(|cx| checkout.read(cx).project().cloned());
    let updates = project
        .expect("the checkout opened")
        .subscribe(Feed::Everything);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        cx.run_until_parked();
        if cx.read(|cx| done(checkout.read(cx))) {
            return;
        }
        let left = deadline.checked_duration_since(Instant::now());
        updates.next_within(left.expect("the changes were never read"));
    }
}

/// Presses `keys` in the changes panel's list, as a user moving through it does.
fn press_in_list(
    window: AnyWindowHandle,
    panel: &Entity<ChangesPanel>,
    keys: &str,
    cx: &mut TestAppContext,
) {
    (cx.update_window(window, |_, window, cx| {
        let focus = panel.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        window.draw(cx).clear(cx);
    }))
    .unwrap();
    cx.simulate_keystrokes(window, keys);
    cx.run_until_parked();
}

/// How many of the tabs show the diffs.
fn diffs_tabs(shell: &Entity<Shell>, cx: &TestAppContext) -> usize {
    (tabs(shell, cx).iter())
        .filter(|tab| tab.trim_matches(['[', ']']) == "Changes")
        .count()
}

#[gpui_kit::test]
fn a_file_picked_in_the_list_shows_the_one_changes_tab_which_comes_back_once_closed(
    cx: &mut TestAppContext,
) {
    let root = repo("changes-tab", &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    write(&root, "a.txt", b"a\nmore\n");
    write(&root, "b.txt", b"b\nmore\n");
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    cx.dispatch_action(window, ToggleChanges);
    cx.run_until_parked();
    let panel = cx.read(|cx| shell.read(cx).right_panel.changes().cloned());
    let panel = panel.expect("the changes panel");
    settle(&panel, cx, |checkout| checkout.files().len() == 2);
    let before = tabs(&shell, cx);
    assert_eq!(
        diffs_tabs(&shell, cx),
        0,
        "nothing's picked yet: {before:?}"
    );

    // Picked, a file opens the diffs right after the tab shown, and shows them.
    press_in_list(window, &panel, "j", cx);
    let opened = tabs(&shell, cx);
    let shown = before
        .iter()
        .position(|tab| tab.starts_with('['))
        .map_or(0, |at| at + 1);
    assert_eq!(
        opened.get(shown).map(String::as_str),
        Some("[Changes]"),
        "{opened:?}"
    );
    // Another file picked: the same tab, scrolled on.
    press_in_list(window, &panel, "k", cx);
    assert_eq!(tabs(&shell, cx), opened);
    // From another tab, picking a file shows the diffs' tab where it is.
    with_shell(window, &shell, cx, |shell, _, cx| {
        let (project, branch, _) = shell.workspace.shown_tab().unwrap();
        shell.workspace.select_tab(project, branch, 0);
        cx.notify();
    });
    cx.run_until_parked();
    press_in_list(window, &panel, "j", cx);
    assert_eq!(tabs(&shell, cx), opened, "shown again, not opened again");

    // Closed, the diffs' tab comes back with ⌘⇧T, showing the diffs.
    with_shell(window, &shell, cx, |shell, window, cx| {
        let at = shell.workspace.shown_tab().unwrap();
        shell.close_tab_at(at, window, cx);
    });
    cx.run_until_parked();
    assert_eq!(diffs_tabs(&shell, cx), 0);
    cx.simulate_keystrokes(window, "cmd-shift-t");
    cx.run_until_parked();
    assert_eq!(tabs(&shell, cx), opened);
    let showing = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().unwrap();
        let tab = &unit.tabs[unit.active_tab];
        tab.panes().iter().any(|pane| pane.changes.is_some())
    });
    assert!(showing, "the tab back shows the diffs");
    let _ = std::fs::remove_dir_all(root);
}
