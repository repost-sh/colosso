//! The Edit menu takes no key from what has the keyboard, and each of its items lands where its
//! key does (`docs/native-layer.md` 7).

use gpui_kit::{Action, AppContext as _, Keystroke, OwnedMenuItem, TestAppContext};

use super::{Copy, Cut, ITEMS, Paste, Redo, SelectAll, Undo};
use crate::test_support::{click_pane, keyboard_in, open_page, repo, select, shell_with};

fn key(source: &'static str) -> Keystroke {
    Keystroke::parse(source).unwrap()
}

#[gpui_kit::test]
fn the_menu_bar_has_an_edit_menu_with_the_systems_six_edits(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.update(|cx| crate::quitting::set_menu(cx));
    let menus = cx.update(|cx| cx.get_menus()).unwrap();
    let edit = menus.iter().find(|menu| menu.name.as_ref() == "Edit").expect("an Edit menu");
    let named: Vec<(String, bool)> = edit
        .items
        .iter()
        .filter_map(|item| match item {
            OwnedMenuItem::Action { name, os_action, .. } => Some((name.clone(), os_action.is_some())),
            _ => None,
        })
        .collect();
    let wanted: Vec<(String, bool)> = ITEMS.iter().map(|item| (item.name.to_owned(), true)).collect();
    assert_eq!(named, wanted, "each of the system's six, by its own name, as the system knows it");
}

/// The menu shows a key for an item only if the keymap binds one to the item's action, and a
/// key it shows it can take. None is bound: the menu can't take ⌘C from a page or an input.
#[gpui_kit::test]
fn no_key_is_bound_to_an_edit_item_so_the_menu_takes_none(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let actions: [&dyn Action; 6] = [&Undo, &Redo, &Cut, &Copy, &Paste, &SelectAll];
        for action in actions {
            let bound = keymap.bindings().filter(|binding| binding.action().partial_eq(action)).count();
            assert_eq!(bound, 0, "{}", action.name());
        }
    });
}

/// With the keyboard in a page, each item does what its key does there: the page's own edit,
/// which makes the page the person's.
#[gpui_kit::test]
fn an_edit_item_picked_with_the_keyboard_in_a_page_does_what_its_key_does(cx: &mut TestAppContext) {
    for by_menu in [false, true] {
        let root = repo(if by_menu { "edit-menu-item" } else { "edit-menu-key" }, &[("README.md", "hi\n")]);
        let (window, shell) = shell_with(&[&root], cx);
        cx.update(|cx| crate::quitting::set_menu(cx));
        select(&shell, &root, cx);
        let pane = open_page(window, &shell, Some("https://example.com/"), cx);
        click_pane(window, pane, cx);
        assert!(keyboard_in(window, "BrowserPage", cx), "the keyboard is in the page");
        let acted = |cx: &mut TestAppContext| {
            shell.read_with(cx, |shell, cx| {
                shell.workspace.pane(pane).unwrap().view().unwrap().read(cx).person_has_it()
            })
        };
        shell.update(cx, |shell, cx| {
            let view = shell.workspace.pane(pane).unwrap().view().unwrap().clone();
            view.update(cx, crate::browser::BrowserView::nobody_has_it);
        });
        assert!(!acted(cx), "nobody has the page yet");
        if by_menu {
            (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
            cx.run_until_parked();
            assert!(!acted(cx), "coming to the front isn't an edit");
            cx.dispatch_action(window, Copy);
            cx.run_until_parked();
        } else {
            cx.simulate_keystrokes(window, "cmd-c");
        }
        assert!(acted(cx), "by the menu: {by_menu}");
    }
}

/// A key Koyo binds nothing to in a page is no more Koyo's with the Edit menu in the bar: ⌘K
/// and ⌘E go on to the page. (⌘B isn't such a key: it's the sidebar's, wherever the keyboard
/// is.)
#[gpui_kit::test]
fn a_key_koyo_binds_nothing_to_in_a_page_is_still_left_to_the_page(cx: &mut TestAppContext) {
    let root = repo("edit-menu-unbound", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    cx.update(|cx| crate::quitting::set_menu(cx));
    select(&shell, &root, cx);
    let pane = open_page(window, &shell, Some("https://example.com/"), cx);
    click_pane(window, pane, cx);
    assert!(keyboard_in(window, "BrowserPage", cx));
    for unbound in ["cmd-k", "cmd-e"] {
        let taken = cx
            .update_window(window, |_, window, cx| window.dispatch_keystroke(key(unbound), cx))
            .unwrap();
        assert!(!taken, "GPUI has no use for {unbound} in a page, and leaves it");
    }
}
