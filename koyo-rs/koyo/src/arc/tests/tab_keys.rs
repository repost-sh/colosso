//! ⌥↑ and ⌥↓ show the tab above and below in the Arc sidebar (`docs/arc-sidebar.md` 7): down its
//! rows, the space's loose pages and then each worktree's tabs, passing over a collapsed
//! worktree's, the keyboard left where it was so they go on, the pill passing from row to row;
//! at the first or last row nothing changes. Where the keyboard is in text they're the text's.

use gpui_kit::{AnyWindowHandle, AppContext as _, Axis, Entity, TestAppContext};

use super::chat;
use super::project_keys::{assert_passing, frames, passing};
use super::space_keys::{TABS_MOVE, pressed};
use super::worktrees::{folder_of, with_worktrees};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, keyboard_at_window, open_page};

/// The row shown: "loose" for a loose page, else the title of the worktree's tab.
fn row(shell: &Entity<Shell>, cx: &TestAppContext) -> String {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        if workspace.loose_shown().is_some() {
            return "loose".into();
        }
        let tab = workspace
            .shown_checkout_tab()
            .and_then(|_| workspace.tab_shown());
        tab.map_or_else(String::new, |tab| tab.first().title.to_string())
    })
}

/// Presses `key` and lets it up: the row shown after.
fn press(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    key: &'static str,
    cx: &mut TestAppContext,
) -> String {
    let (action, contexts) = pressed(window, key, cx);
    let expected = TABS_MOVE[usize::from(key == "alt-down")];
    assert_eq!(action, expected, "{key} in {contexts:?}");
    row(shell, cx)
}

#[gpui_kit::test]
fn option_up_and_down_go_down_the_sidebars_rows_and_leave_the_keyboard_be(cx: &mut TestAppContext) {
    let (window, shell, root) = with_worktrees("tab-keys", cx);
    shell.update(cx, |shell, cx| {
        for (branch, titles) in [(0, &["m1", "m2"][..]), (1, &["f1"]), (2, &["x1"])] {
            shell.workspace.select_branch(0, branch);
            for title in titles {
                shell.workspace.push_tab(chat(title));
            }
        }
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    // A loose page of the space, over the worktrees: a page pinned and then loosened.
    let page = open_page(window, &shell, Some("https://example.com/loose"), cx);
    shell.update(cx, |shell, cx| {
        assert!(shell.pin_tab_of(page, cx));
        shell.loosen_pinned(page, 0, cx);
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    assert_eq!(row(&shell, cx), "m1");

    let go = |key, cx: &mut TestAppContext| press(window, &shell, key, cx);
    assert_eq!(
        go("alt-up", cx),
        "loose",
        "the loose page, over the worktrees"
    );
    // A page shown this way doesn't take the keyboard, so ⌥↑ is still heard, with no row above.
    assert_eq!(go("alt-up", cx), "loose", "nothing above the first row");
    let down: Vec<_> = (0..5).map(|_| go("alt-down", cx)).collect();
    assert_eq!(
        down,
        ["m1", "m2", "f1", "x1", "x1"],
        "down the worktrees, to the last"
    );
    assert!(keyboard_at_window(window, &shell, cx));

    // `feat` collapsed: its tab is passed over.
    shell.update(cx, |shell, cx| {
        let feat = folder_of(shell, "feat");
        settings::update(cx, |settings| settings.toggle_collapsed(&root, &feat));
    });
    draw(window, cx);
    assert_eq!(go("alt-up", cx), "m2", "past the collapsed worktree");
    assert_eq!(go("alt-down", cx), "x1");
}

/// With motion, ⌥↓ passes the pill out of the bottom of the row it leaves and into the top of
/// the next, as far in the one as it's gone out of the other, until it's the row's own again;
/// ⌥↑ the other way about.
#[gpui_kit::test]
fn with_motion_the_pill_passes_out_of_the_row_left_and_into_the_next(cx: &mut TestAppContext) {
    let (window, shell, _) = with_worktrees("tab-keys-pill", cx);
    let panes = shell.update(cx, |shell, cx| {
        shell.workspace.select_branch(0, 0);
        shell.workspace.push_tab(chat("a"));
        shell.workspace.push_tab(chat("b"));
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
        let tabs = &shell.workspace.projects[0].branches[0].tabs;
        [tabs[0].first().id, tabs[1].first().id]
    });
    draw(window, cx);
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    cx.update(|cx| cx.set_reduce_motion(false));
    // The first worktree's tab at `tab`, its row known by its place and its pill by its pane.
    let pill = |tab: usize, cx: &mut TestAppContext| {
        let drawn_as = (
            format!("arc-tab-light-{}", panes[tab]),
            format!("tab-0-0-{tab}"),
        );
        passing(window, drawn_as, Axis::Vertical, cx)
    };
    let both = |cx: &mut TestAppContext| (pill(0, cx), pill(1, cx));
    assert_eq!(both(cx), (None, None), "at rest, the row's own pill");

    assert_eq!(press(window, &shell, "alt-down", cx), "b");
    frames(window, 3, cx);
    let (going, coming) = both(cx);
    assert_passing(going, coming, true);
    frames(window, 30, cx);
    assert_eq!(both(cx), (None, None));

    assert_eq!(press(window, &shell, "alt-up", cx), "a");
    frames(window, 3, cx);
    let (coming, going) = both(cx);
    assert_passing(going, coming, false);
    frames(window, 30, cx);
    assert_eq!(both(cx), (None, None));
}
