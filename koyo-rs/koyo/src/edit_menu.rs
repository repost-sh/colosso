//! The menu bar's Edit menu (`docs/native-layer.md` 7). `AppKit` puts items of its own only in
//! a menu titled Edit: Emoji & Symbols, Start Dictation, `AutoFill`. Its own six, Undo to Select
//! All, each do what their key does wherever the keyboard is, by pressing that key: one way in
//! for an edit, whether a hand or the menu asks for it.
//!
//! None of the six has a key bound to it in the keymap, so the menu shows none and takes none:
//! `AppKit` asks the window's views for a key before the menu, and a key Koyo binds nothing to
//! in a page goes on to the page as before.

use gpui_kit::{Action, App, Keystroke, Menu, MenuItem, OsAction, actions};

actions!(edit_menu, [Undo, Redo, Cut, Copy, Paste, SelectAll]);

/// An item of the menu: its name, the key that does the same, and what the system calls it.
pub struct Item {
    pub name: &'static str,
    pub key: &'static str,
    pub os: OsAction,
}

/// The menu's own items, in the order macOS lists them.
pub const ITEMS: [Item; 6] = [
    Item { name: "Undo", key: "cmd-z", os: OsAction::Undo },
    Item { name: "Redo", key: "cmd-shift-z", os: OsAction::Redo },
    Item { name: "Cut", key: "cmd-x", os: OsAction::Cut },
    Item { name: "Copy", key: "cmd-c", os: OsAction::Copy },
    Item { name: "Paste", key: "cmd-v", os: OsAction::Paste },
    Item { name: "Select All", key: "cmd-a", os: OsAction::SelectAll },
];

fn action(os: OsAction) -> Box<dyn Action> {
    match os {
        OsAction::Undo => Box::new(Undo),
        OsAction::Redo => Box::new(Redo),
        OsAction::Cut => Box::new(Cut),
        OsAction::Copy => Box::new(Copy),
        OsAction::Paste => Box::new(Paste),
        OsAction::SelectAll => Box::new(SelectAll),
    }
}

/// The menu, for the menu bar: Undo and Redo, then the rest, as macOS lists them. `AppKit`
/// adds its own items after them.
pub fn menu() -> Menu {
    let mut items = Vec::new();
    for (at, item) in ITEMS.iter().enumerate() {
        if at == 2 {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::Action {
            name: item.name.into(),
            action: action(item.os),
            os_action: Some(item.os),
            checked: false,
            disabled: false,
        });
    }
    Menu::new("Edit").items(items)
}

/// An item picked presses its key in the window in front, once the menu's own pick is done
/// with: what the key does there, the item does.
fn press(key: &'static str, cx: &mut App) {
    let (Some(window), Ok(key)) = (cx.active_window(), Keystroke::parse(key)) else {
        return;
    };
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| window.dispatch_keystroke(key, cx));
    });
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Undo, cx| press(ITEMS[0].key, cx));
    cx.on_action(|_: &Redo, cx| press(ITEMS[1].key, cx));
    cx.on_action(|_: &Cut, cx| press(ITEMS[2].key, cx));
    cx.on_action(|_: &Copy, cx| press(ITEMS[3].key, cx));
    cx.on_action(|_: &Paste, cx| press(ITEMS[4].key, cx));
    cx.on_action(|_: &SelectAll, cx| press(ITEMS[5].key, cx));
}

#[cfg(test)]
mod tests;
