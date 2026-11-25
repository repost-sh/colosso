//! ⌥↑ and ⌥↓ show the tab above and below in the sidebar (`docs/sidebar.md` 6): down its
//! rows, the space's loose pages and then each worktree's tabs, passing over a collapsed
//! worktree's, the keyboard left where it was so they go on; at the first or last row nothing
//! changes. Where the keyboard is in text they're the text's.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::chat;
use super::space_keys::TABS_MOVE;
use super::branches::{folder_of, with_worktrees};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, keyboard_at_window, open_page, pressed};

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
    let page = open_page(window, &shell, Some("https://example.com/loose"), cx);
    shell.update(cx, |shell, cx| {
        assert!(shell.pin_tab_of(page, usize::MAX, cx));
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
    assert_eq!(go("alt-up", cx), "loose", "nothing above the first row");
    let down: Vec<_> = (0..5).map(|_| go("alt-down", cx)).collect();
    assert_eq!(
        down,
        ["m1", "m2", "f1", "x1", "x1"],
        "down the worktrees, to the last"
    );
    assert!(keyboard_at_window(window, &shell, cx));

    shell.update(cx, |shell, cx| {
        let feat = folder_of(shell, "feat");
        settings::update(cx, |settings| settings.toggle_collapsed(&root, &feat));
    });
    draw(window, cx);
    assert_eq!(go("alt-up", cx), "m2", "past the collapsed worktree");
    assert_eq!(go("alt-down", cx), "x1");
}

/// With motion, ⌥↓ and ⌥↑ show the row below and the row above at once, as without it: the
/// highlight is on the new row as soon as the key is up.
#[gpui_kit::test]
fn with_motion_the_row_below_and_above_are_shown_at_once(cx: &mut TestAppContext) {
    let (window, shell, _) = with_worktrees("tab-keys-motion", cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.select_branch(0, 0);
        shell.workspace.push_tab(chat("a"));
        shell.workspace.push_tab(chat("b"));
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    (cx.update_window(window, |_, window, cx| window.blur(cx))).unwrap();
    cx.update(|cx| cx.set_reduce_motion(false));
    assert_eq!(press(window, &shell, "alt-down", cx), "b");
    assert_eq!(press(window, &shell, "alt-up", cx), "a");
}
