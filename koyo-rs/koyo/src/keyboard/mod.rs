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

use gpui_kit::{AnyWindowHandle, App, Window};

#[cfg(all(target_os = "macos", not(test)))]
mod appkit;
mod hooks;
mod lines;
mod rate;
mod reconcile;
mod state;
#[cfg(test)]
mod tests;

pub use state::{Cause, Holder, Intent, PaneId, Part, Surface};
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
    if changes.iter().any(|change| change.cause != Cause::Click) {
        hooks::end_click();
    }
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
    if cause != Cause::Click {
        hooks::end_click();
    }
    change(|keyboards| keyboards.set(window, surface, cause));
}

/// `window` shows the tab known by `tab` now, and hands it the keyboard: what that tab had
/// the keyboard in when it was last left, if it's known.
pub fn tab_shown(window: &Window, tab: u64) -> Option<Surface> {
    let window = known(window);
    KEYBOARDS.with(|keyboards| keyboards.borrow_mut().tab_shown(window, tab))
}

/// GPUI's focus moved into the page in `pane`, the view whose focus is the page's.
pub fn entered_page(window: &Window, pane: PaneId) {
    set(window, Surface::Page(pane), Cause::Focused);
}

/// GPUI's focus left the page in `pane` for `part` of Koyo's own.
pub fn left_page(window: &Window, pane: PaneId, part: Part) {
    let window = known(window);
    change(|keyboards| keyboards.left_page(window, pane, part));
}

/// `window` became the key window, or stopped being it; and GPUI's focus is where it is now
/// (`follow_focus`). Read as the window's key state changes and as each frame of the shell is
/// drawn, so a change the holders missed is made up.
pub fn key(window: &Window, cx: &mut App) {
    follow_focus(window, cx);
    let key = window.is_window_active();
    let window = known(window);
    change(|keyboards| keyboards.key(window, key));
}

/// A page the person put the keyboard in has it only while GPUI's focus is its view's: if it
/// isn't, the keyboard is in GPUI's view, its bar or elsewhere. GPUI tells a page its focus
/// left only in a frame after one drawn with the window key, so a move made while it wasn't,
/// by the very press that makes it key again (back from the launcher or another app, on the
/// composer), is never told.
pub fn follow_focus(window: &Window, cx: &mut App) {
    if !window.is_window_active() {
        return;
    }
    let key = known(window);
    let Some((_, Surface::Page(pane))) = held(key) else {
        return;
    };
    let view = crate::browser::page_view(pane, cx);
    let Some(part) = (match &view {
        Some(view) => view.read(cx).part_with_focus(window, cx),
        None => Some(Part::Other),
    }) else {
        return;
    };
    change(|keyboards| keyboards.left_page(key, pane, part));
}

/// The page in `pane` closed.
pub fn page_closed(pane: PaneId) {
    change(|keyboards| keyboards.page_closed(pane));
}

/// `window`, a floating window of Koyo's (a menu, a dialog, the launcher, the tab switcher's
/// panel), belongs to `over`, which has the keys back if it closes with them.
pub fn opened_over(window: &Window, over: AnyWindowHandle) {
    #[cfg(all(target_os = "macos", not(test)))]
    appkit::over(window, key_of(over));
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = (window, over);
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
