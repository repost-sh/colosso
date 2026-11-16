//! Where the person's keyboard is in each of Koyo's windows (`docs/keyboard.md`).
//!
//! A window has one holder at a time: GPUI's view, with the part of Koyo's in it that GPUI's
//! focus is in (a chat's composer, a terminal, an editor, the launcher's field, the agent
//! desktop); a page's own views, Chromium's (a pinned, loose or worktree page, `DevTools`);
//! a native menu while it tracks the keys; or nothing, away, while another window or app has
//! them. Everything that moves the keyboard is told here with its cause (`state.rs`), and the
//! log gets one line for each move (`lines.rs`), naming surfaces by kind and pane and never
//! anything typed.
//!
//! It's on the main thread, where `AppKit` and GPUI are, and is read and changed there only.

use std::cell::RefCell;

use gpui_kit::{AnyWindowHandle, FocusHandle, Window};

mod lines;
mod state;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use state::Holder;
pub use state::{Cause, PaneId, Part, Surface};
use state::{Change, Keyboards, WindowKey};

thread_local! {
    static KEYBOARDS: RefCell<Keyboards> = RefCell::default();
}

/// GPUI's window as the holders know it.
fn key_of(window: AnyWindowHandle) -> WindowKey {
    window.window_id().as_u64()
}

/// Runs `with` on the holders, and logs the changes it made.
fn change<C: IntoIterator<Item = Change>>(with: impl FnOnce(&mut Keyboards) -> C) {
    let changes = KEYBOARDS.with(|keyboards| with(&mut keyboards.borrow_mut()));
    for change in changes {
        lines::tell(&change);
    }
}

/// The person's keyboard in `window` is in `surface` now, because of `cause`.
pub fn set(window: &Window, surface: Surface, cause: Cause) {
    let window = key_of(window.window_handle());
    change(|keyboards| keyboards.set(window, surface, cause));
}

/// GPUI's focus moved into the page in `pane`, the view whose focus is the page's.
pub fn entered_page(window: &Window, pane: PaneId) {
    set(window, Surface::Page(pane), Cause::Focused);
}

/// GPUI's focus left the page in `pane` (whose focus is `focus`) for `part` of Koyo's own.
/// Not when the window only stopped being key: GPUI's focus stays where it was then, though
/// its views hear it leave.
pub fn left_page(window: &Window, pane: PaneId, focus: &FocusHandle, part: Part) {
    if focus.is_focused(window) {
        return;
    }
    let window = key_of(window.window_handle());
    change(|keyboards| keyboards.left_page(window, pane, part));
}

/// `window` became the key window, or stopped being it.
pub fn key(window: &Window) {
    let key = window.is_window_active();
    let window = key_of(window.window_handle());
    change(|keyboards| keyboards.key(window, key));
}

/// The page in `pane` closed.
pub fn page_closed(pane: PaneId) {
    change(|keyboards| keyboards.page_closed(pane));
}

/// A dialog of Koyo's open over a window, for as long as it's kept: it has that window's keys.
pub struct DialogOver(AnyWindowHandle);

impl DialogOver {
    pub fn new(over: AnyWindowHandle) -> DialogOver {
        let window = key_of(over);
        change(|keyboards| keyboards.dialog(window, true));
        DialogOver(over)
    }
}

impl Drop for DialogOver {
    fn drop(&mut self) {
        let window = key_of(self.0);
        change(|keyboards| keyboards.dialog(window, false));
    }
}

/// What has the keyboard in `window`, as a test reads it.
#[cfg(test)]
pub fn holder(window: AnyWindowHandle) -> Option<Holder> {
    KEYBOARDS.with(|keyboards| keyboards.borrow().holder(key_of(window)))
}
