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
//! The holders are the one say over `AppKit`'s first responder, which only the reconciler
//! moves (`reconcile.rs`), told by the window of each change, each press and each key
//! (`hooks.rs`, `appkit.rs`, the vendored gpui-fast-macos's `responders`).
//!
//! It's on the main thread, where `AppKit` and GPUI are, and is read and changed there only.

use std::cell::RefCell;

use gpui_kit::{AnyWindowHandle, App, FocusHandle, Window};

#[cfg(all(target_os = "macos", not(test)))]
mod appkit;
mod hooks;
mod lines;
mod reconcile;
mod state;
#[cfg(test)]
mod tests;

pub use state::{Cause, Holder, PaneId, Part, Surface};
use state::{Change, Keyboards, WindowKey};

thread_local! {
    static KEYBOARDS: RefCell<Keyboards> = RefCell::default();
}

/// Has the window's say over the keyboard go through the holders, in the app; tests, with no
/// `AppKit` of their own, only keep the holders.
pub fn init(cx: &mut App) {
    #[cfg(all(target_os = "macos", not(test)))]
    appkit::install(cx);
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = cx;
}

/// GPUI's window as the holders know it.
fn key_of(window: AnyWindowHandle) -> WindowKey {
    window.window_id().as_u64()
}

/// GPUI's `window` as the holders know it, which `AppKit`'s side learns of the first time.
fn known(window: &Window) -> WindowKey {
    #[cfg(all(target_os = "macos", not(test)))]
    appkit::know(window);
    key_of(window.window_handle())
}

/// What has the keyboard in `window`, and what the person put it in there, if it's known.
fn held(window: WindowKey) -> Option<(Holder, Surface)> {
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
    for change in changes {
        let moved = hooks::reconcile(change.window);
        lines::tell(&change, moved);
    }
}

/// The person's keyboard in `window` is in `surface` now, because of `cause`.
pub fn set(window: &Window, surface: Surface, cause: Cause) {
    set_in(known(window), surface, cause);
}

fn set_in(window: WindowKey, surface: Surface, cause: Cause) {
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
    let window = known(window);
    change(|keyboards| keyboards.left_page(window, pane, part));
}

/// `window` became the key window, or stopped being it.
pub fn key(window: &Window) {
    let key = window.is_window_active();
    let window = known(window);
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
