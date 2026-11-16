//! ⌃⌘Space opens the system's emoji and symbols wherever the keyboard is. `AppKit` gives an app
//! that key through the Emoji & Symbols item it adds to a menu titled Edit, and Koyo's menu bar
//! has no such menu (`docs/native-layer.md` 7): gpui-kit's own inputs bind the key themselves,
//! and with the keyboard in a browser page's field nothing took it, so nothing opened. Bound
//! for the whole app, it's offered to Koyo's bindings before a page hears it, as every ⌘ key
//! Koyo binds is, and what's picked is typed into whatever has the keyboard, the page's field
//! too.

use gpui_kit::{App, KeyBinding, actions};

actions!(keyboard, [ShowCharacters]);

/// The key, as GPUI's keymap writes it.
pub const KEY: &str = "ctrl-cmd-space";

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(KEY, ShowCharacters, None)]);
    cx.on_action(|_: &ShowCharacters, cx| {
        let Some(window) = cx.active_window() else {
            return;
        };
        // The window is busy with the key that asked: once it's done.
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, _| window.show_character_palette());
        });
    });
}
