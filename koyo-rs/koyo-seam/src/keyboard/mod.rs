//! Where the person's keyboard is in each of the app's windows (`docs/keyboard.md`).
//!
//! A window has one holder at a time: GPUI's view, with the part of the app's in it that GPUI's
//! focus is in (a chat's composer, a terminal, an editor, the launcher's field, the agent
//! desktop); a page's own views, Chromium's (a pinned, loose or worktree page, `DevTools`);
//! a native menu while it tracks the keys; or nothing, away, while another window or app has
//! them. Everything that moves the keyboard is told here with its cause (`state.rs`), and the
//! log gets one line for each move (`lines.rs`), naming surfaces by kind and pane and never
//! anything typed.
//!
//! The holders are the one say over `AppKit`'s first responder, which only the reconciler
//! moves (`reconcile.rs`), told by the window of each change, each press and each key
//! (`hooks.rs`). The app tells them of GPUI's windows and focus, and gives them `AppKit`.
//!
//! It's on the main thread, where `AppKit` and GPUI are, and is read and changed there only.

use std::cell::RefCell;

pub mod hooks;
mod lines;
pub mod rate;
pub mod reconcile;
pub mod state;
#[cfg(any(test, feature = "testing"))]
#[path = "tests/fake.rs"]
pub mod testing;
#[cfg(test)]
mod tests;

pub use state::{Cause, Holder, Intent, PaneId, Part, Surface};
use state::{Change, Keyboards, WindowKey};

thread_local! {
    static KEYBOARDS: RefCell<Keyboards> = RefCell::default();
}

/// What has the keyboard in `window`, and what the person put it in there, if it's known.
#[must_use]
pub fn held(window: WindowKey) -> Option<(Holder, Surface)> {
    KEYBOARDS.with(|keyboards| {
        let keyboards = keyboards.borrow();
        Some((keyboards.holder(window)?, keyboards.surface(window)?))
    })
}

/// Runs `with` on the holders; each change it made moves the first responder to match, and is
/// logged with how it moved.
fn change<C: IntoIterator<Item = Change>>(with: impl FnOnce(&mut Keyboards) -> C) {
    let changes: Vec<Change> =
        KEYBOARDS.with(|keyboards| with(&mut keyboards.borrow_mut()).into_iter().collect());
    if changes.iter().any(|change| change.cause != Cause::Click) {
        hooks::end_click();
    }
    for change in changes {
        let moved = hooks::reconcile(change.window);
        lines::tell(&change, moved);
    }
}

/// The person's keyboard in `window` is in `surface` now, because of `cause`.
pub fn set_in(window: WindowKey, surface: Surface, cause: Cause) {
    if cause != Cause::Click {
        hooks::end_click();
    }
    change(|keyboards| keyboards.set(window, surface, cause));
}

/// `window` shows the tab known by `tab` now, and hands it the keyboard: what that tab had
/// the keyboard in when it was last left, if it's known.
pub fn tab_shown(window: WindowKey, tab: u64) -> Option<Surface> {
    KEYBOARDS.with(|keyboards| keyboards.borrow_mut().tab_shown(window, tab))
}

/// GPUI's focus left the page in `pane`, in `window`, for `part` of the app's own.
pub fn left_page(window: WindowKey, pane: PaneId, part: Part) {
    change(|keyboards| keyboards.left_page(window, pane, part));
}

/// `window` is the key window, or isn't.
pub fn key(window: WindowKey, key: bool) {
    change(|keyboards| keyboards.key(window, key));
}

/// The page in `pane` closed.
pub fn page_closed(pane: PaneId) {
    change(|keyboards| keyboards.page_closed(pane));
}

/// A dialog of the app's opened over `window`, or closed: open, it has that window's keys.
pub fn dialog(window: WindowKey, open: bool) {
    change(|keyboards| keyboards.dialog(window, open));
}

/// What has the keyboard in `window`.
#[must_use]
pub fn holder(window: WindowKey) -> Option<Holder> {
    KEYBOARDS.with(|keyboards| keyboards.borrow().holder(window))
}
