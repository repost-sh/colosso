//! ⌃⌘Space is Koyo's wherever the keyboard is (`keyboard/characters.rs`).

use gpui_kit::{Keystroke, TestAppContext};

use crate::keyboard::characters::{KEY, ShowCharacters};

/// The key is bound for the whole app, in no part's context: with the keyboard in a browser
/// page, whose own context binds nothing to it, it still finds its action.
#[gpui_kit::test]
fn the_emoji_key_is_bound_for_the_whole_app(cx: &mut TestAppContext) {
    cx.update(crate::init);
    cx.update(|cx| {
        let wanted = Keystroke::parse(KEY).unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bound = keymap.bindings().any(|binding| {
            let [stroke] = binding.keystrokes() else {
                return false;
            };
            let stroke = stroke.inner();
            stroke.modifiers == wanted.modifiers
                && stroke.key == wanted.key
                && binding.predicate().is_none()
                && binding.action().partial_eq(&ShowCharacters)
        });
        assert!(bound, "{KEY} opens the emoji and symbols with no context asked");
    });
}
