//! A press outside the composer's box takes the keyboard out of it, to the window, where it
//! stays until the person presses in the box again or picks a tab (`shell::KeysToWindow`): the
//! chat's background, its messages, the glass round it. Pressed into another field or view, the
//! keyboard is that one's already, and stays there.

use gpui_kit::{Context, Focusable as _, MouseDownEvent, Window};

use super::ChatComposer;
use crate::shell::KeysToWindow;

impl ChatComposer {
    pub(super) fn pressed_outside(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The suggestions are the box's own, over it.
        if self.suggesting() || !self.focus_handle(cx).is_focused(window) {
            return;
        }
        window.dispatch_action(Box::new(KeysToWindow), cx);
    }
}
