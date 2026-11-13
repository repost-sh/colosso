//! The keyboard goes where the person puts it, and stays out of a pane until they do: out of
//! a chat's prompt whenever they only move to the chat.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, Focusable as _, TestAppContext};

use crate::model::PaneKind;
use crate::shell::{CloseTab, Shell};
use crate::test_support::{
    draw, keyboard_at_window, open_page, press_prompt, repo, select, shell_with,
};

/// A shell over a checkout of its own, selected.
fn shell(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    (window, shell)
}

/// A new chat's tab, as the launcher opens one (`Shell::new_tab`): its pane.
fn new_chat(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) -> u64 {
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    draw(window, cx);
    shell.read_with(cx, |shell, _| shell.shown_tab_key().unwrap())
}

/// The pane with the keyboard after a few frames, where one that would take it has had its
/// chance: `None` with the keyboard at the window, or anywhere else.
fn typing_in(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) -> Option<u64> {
    for _ in 0..3 {
        draw(window, cx);
    }
    let pane = cx.update_window(window, |_, window, cx| {
        shell.read(cx).pane_with_keyboard(window, cx)
    });
    pane.unwrap()
}

/// A page's tab, opened at an address as the launcher opens one: its pane, with the keyboard.
fn new_page(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) -> u64 {
    let pane = open_page(window, shell, Some("https://example.com/"), cx);
    draw(window, cx);
    pane
}

/// Puts the keyboard at the window, as a press on the sidebar or the glass does.
fn keys_to_window(window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    }))
    .unwrap();
    assert!(keyboard_at_window(window, shell, cx));
}

/// Moving to a chat, however it's done, never puts the keyboard in its prompt: only a press
/// in the prompt does, or opening a chat to type in.
#[gpui_kit::test]
fn moving_to_a_chat_never_puts_the_keyboard_in_its_prompt(cx: &mut TestAppContext) {
    let (window, shell) = shell("keys-chat-moves", cx);
    let first = new_chat(window, &shell, cx);
    assert_eq!(
        typing_in(window, &shell, cx),
        Some(first),
        "opened to type in"
    );
    let second = new_chat(window, &shell, cx);
    assert_eq!(
        typing_in(window, &shell, cx),
        Some(second),
        "opened to type in"
    );
    let at_window = |how: &str, cx: &mut TestAppContext| {
        assert!(keyboard_at_window(window, &shell, cx), "{how}");
        let shown = shell.read_with(cx, |shell, _| shell.shown_tab_key().unwrap());
        assert!(shown == first || shown == second, "a chat is shown: {how}");
    };

    // Its tab picked in the sidebar, from the other chat's prompt.
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    at_window("its tab picked", cx);
    // Pressed in its prompt, the person types there; ⌃⇥ to the other chat, and they don't.
    press_prompt(window, cx);
    assert_eq!(
        typing_in(window, &shell, cx),
        Some(first),
        "pressed in its prompt"
    );
    cx.simulate_keystrokes(window, "ctrl-tab");
    let (switcher, _) =
        crate::files::tests::dialog::<super::super::switcher::Switcher>(cx).expect("the switcher");
    cx.simulate_keystrokes(switcher, "enter");
    assert_eq!(
        shell.read_with(cx, |shell, _| shell.shown_tab_key()),
        Some(second),
        "the other chat is shown"
    );
    at_window("switched to with ⌃⇥", cx);
    // Asked for by its pane, as the launcher's list of open tabs asks.
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.focus_pane(first, window, cx));
    }))
    .unwrap();
    at_window("asked for by its pane", cx);
    // The settings left from its prompt.
    press_prompt(window, cx);
    cx.simulate_keystrokes(window, "cmd-,");
    draw(window, cx);
    cx.simulate_keystrokes(window, "escape");
    at_window("back from the settings", cx);
    // The chat after it closed from its own prompt, and opened again.
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 1, cx));
    press_prompt(window, cx);
    assert_eq!(typing_in(window, &shell, cx), Some(second));
    cx.simulate_keystrokes(window, "cmd-w");
    at_window("shown in place of the tab closed", cx);
    cx.simulate_keystrokes(window, "cmd-shift-t");
    at_window("opened again", cx);
}

#[gpui_kit::test]
fn a_tab_closed_with_the_keyboard_in_it_hands_it_to_the_tab_shown_in_its_place(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell("keys-close", cx);
    let first = new_page(window, &shell, cx);
    let second = new_page(window, &shell, cx);
    let third = new_page(window, &shell, cx);
    assert_eq!(typing_in(window, &shell, cx), Some(third), "a new page");

    // ⌘W in the page: the page shown in its place has the keyboard.
    cx.simulate_keystrokes(window, "cmd-w");
    assert_eq!(typing_in(window, &shell, cx), Some(second));

    // With the keyboard at the window, the tab shown next leaves it there.
    keys_to_window(window, &shell, cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(CloseTab), cx);
    })
    .unwrap();
    assert!(
        keyboard_at_window(window, &shell, cx),
        "it wasn't in the tab closed"
    );
    let shown = shell.read_with(cx, |shell, _| shell.shown_tab_key().unwrap());
    assert_eq!(shown, first);

    // ⌘⇧T brings a tab back with the keyboard in it.
    cx.simulate_keystrokes(window, "cmd-shift-t");
    assert_eq!(typing_in(window, &shell, cx), Some(second), "opened again");
}

#[gpui_kit::test]
fn the_settings_give_the_keyboard_back_to_the_pane_that_had_it(cx: &mut TestAppContext) {
    let (window, shell) = shell("keys-settings", cx);
    let page = new_page(window, &shell, cx);
    assert_eq!(typing_in(window, &shell, cx), Some(page));

    // ⌘, from the page and Esc on the settings page: back in the page.
    cx.simulate_keystrokes(window, "cmd-,");
    assert_eq!(typing_in(window, &shell, cx), None, "on a settings page");
    cx.simulate_keystrokes(window, "escape");
    assert_eq!(typing_in(window, &shell, cx), Some(page), "back from them");

    // With the keyboard at the window they leave it there.
    keys_to_window(window, &shell, cx);
    cx.simulate_keystrokes(window, "cmd-,");
    draw(window, cx);
    cx.simulate_keystrokes(window, "escape");
    assert!(
        keyboard_at_window(window, &shell, cx),
        "it was at the window before"
    );
}

/// Another project, or another worktree, shown instead doesn't put the keyboard in its tab,
/// a page's as little as a chat's.
#[gpui_kit::test]
fn another_project_shown_leaves_the_keyboard_at_the_window(cx: &mut TestAppContext) {
    let app = repo("keys-project-app", &[("README.md", "hi\n")]);
    let site = repo("keys-project-site", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&app, &site], cx);
    select(&shell, &site, cx);
    new_page(window, &shell, cx);
    select(&shell, &app, cx);
    let chat = new_chat(window, &shell, cx);
    assert_eq!(
        typing_in(window, &shell, cx),
        Some(chat),
        "typing in the app's new chat"
    );

    // The other project's tile pressed, and the first's again: neither tab takes it.
    shell.update(cx, |shell, cx| shell.select_project(1, cx));
    assert!(
        keyboard_at_window(window, &shell, cx),
        "the site's page is shown"
    );
    shell.update(cx, |shell, cx| shell.select_project(0, cx));
    assert!(
        keyboard_at_window(window, &shell, cx),
        "the app's chat is shown again"
    );
}

/// Its keys, ⌘R, ⌘F and the rest, are bound where the page has the keyboard.
#[gpui_kit::test]
fn a_page_takes_the_keyboard_when_its_tab_is_picked_in_the_sidebar(cx: &mut TestAppContext) {
    let root = repo("shell-page-keyboard", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let pane = open_page(window, &shell, Some("https://example.com/"), cx);
    let page_has_keyboard = |cx: &mut TestAppContext| {
        // Drawn, the tab asked to takes the keyboard; drawn again, its keys are bound.
        for _ in 0..3 {
            draw(window, cx);
        }
        cx.update_window(window, |_, window, cx| {
            let shell = shell.read(cx);
            let view = shell.workspace.branch()?.pane(pane)?.view()?.clone();
            let context = window.context_stack();
            let bound = context
                .iter()
                .any(|context| context.contains("BrowserPage"));
            Some(view.focus_handle(cx).is_focused(window) && bound)
        })
        .unwrap()
    };
    assert_eq!(page_has_keyboard(cx), Some(true), "a new tab at an address");

    // A chat takes the keyboard; its tab left for the page's, as a click on its row leaves
    // it, the page has it again.
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    assert_eq!(page_has_keyboard(cx), Some(false));
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    assert_eq!(page_has_keyboard(cx), Some(true));
    let focused = cx.update_window(window, |_, window, cx| {
        shell.read(cx).focused_pane(window, cx)
    });
    assert_eq!(focused.unwrap(), Some(pane));
}

#[gpui_kit::test]
fn the_keyboard_comes_back_to_the_file_the_person_was_in_not_to_the_chat_beside_it(
    cx: &mut TestAppContext,
) {
    let root = repo("shell-file-keyboard", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    // A chat on the left, the file on its right; and a tab after them.
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.new_tab(PaneKind::Chat, cx);
            shell.open_file(
                crate::files::Place::file("README.md"),
                true,
                false,
                window,
                cx,
            );
            let unit = shell.workspace.branch_mut().expect("a checkout");
            let chat = unit.tabs[0].first().id;
            assert!(unit.drop_tab(1, chat, crate::model::Edge::Right));
            cx.notify();
        });
    })
    .unwrap();
    let (chat, file) = shell.read_with(cx, |shell, _| {
        let panes = shell.workspace.tab_shown().expect("the split").panes();
        (panes[0].id, panes[1].id)
    });
    let settled = |cx: &mut TestAppContext| {
        for _ in 0..3 {
            draw(window, cx);
        }
        let focused = cx.update_window(window, |_, window, cx| {
            shell.read(cx).focused_pane(window, cx)
        });
        focused.unwrap()
    };
    // Leaving the tab and coming back, as a click on its row does: a file only looked at
    // leaves the keyboard to the chat.
    let away_and_back = |cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
        draw(window, cx);
        shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    };
    away_and_back(cx);
    assert_eq!(settled(cx), Some(chat));

    // In the file, as a press in it puts them: it has the keyboard again on the way back.
    shell.update(cx, |shell, cx| shell.enter_pane(file, cx));
    away_and_back(cx);
    assert_eq!(settled(cx), Some(file), "the file they were in");
    let typing = cx.update_window(window, |_, window, cx| {
        let shell = shell.read(cx);
        let pane = shell.workspace.branch().and_then(|unit| unit.pane(file));
        pane.and_then(|pane| pane.file.clone())
            .is_some_and(|file| file.read(cx).has_focus(window, cx))
    });
    assert!(typing.unwrap(), "its editor has the keyboard");
}

/// Two pages side by side in one tab: their panes, left then right.
fn two_pages_beside(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) -> (u64, u64) {
    new_page(window, shell, cx);
    new_page(window, shell, cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        let left = unit.tabs[0].first().id;
        assert!(unit.drop_tab(1, left, crate::model::Edge::Right));
        cx.notify();
    });
    draw(window, cx);
    shell.read_with(cx, |shell, _| {
        let panes = shell.workspace.tab_shown().expect("the split").panes();
        (panes[0].id, panes[1].id)
    })
}

#[gpui_kit::test]
fn a_pane_closed_with_the_keyboard_in_it_hands_it_to_the_pane_left_beside_it(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell("keys-close-pane", cx);
    let (left, right) = two_pages_beside(window, &shell, cx);
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.focus_pane(right, window, cx));
    }))
    .unwrap();
    assert_eq!(typing_in(window, &shell, cx), Some(right));
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.close_pane(right, window, cx));
    }))
    .unwrap();
    assert_eq!(
        typing_in(window, &shell, cx),
        Some(left),
        "the one left beside it"
    );
}

#[gpui_kit::test]
fn a_pane_closed_with_the_keyboard_at_the_window_leaves_it_there(cx: &mut TestAppContext) {
    let (window, shell) = shell("keys-close-pane-window", cx);
    let (_, right) = two_pages_beside(window, &shell, cx);
    keys_to_window(window, &shell, cx);
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.close_pane(right, window, cx));
    }))
    .unwrap();
    assert!(
        keyboard_at_window(window, &shell, cx),
        "it wasn't in the pane closed"
    );
}
