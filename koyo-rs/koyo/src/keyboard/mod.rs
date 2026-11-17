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
//! (`hooks.rs`, `appkit.rs`, the vendored gpui-fast-macos's `responders`). The holders and their
//! rules are the native layer's (`koyo-seam`, `keyboard`); here GPUI's windows and focus are
//! told to them, and `AppKit` is given them.
//!
//! It's on the main thread, where `AppKit` and GPUI are, and is read and changed there only.

use gpui_kit::{AnyWindowHandle, App, Window};
use koyo_seam::keyboard as seam;
use koyo_seam::keyboard::state::WindowKey;

#[cfg(all(target_os = "macos", not(test)))]
mod appkit;
#[cfg(target_os = "macos")]
mod characters;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use koyo_seam::keyboard::Holder;
pub use koyo_seam::keyboard::{Cause, Intent, PaneId, Part, Surface};

/// Has the window's say over the keyboard go through the holders, in the app; tests, with no
/// `AppKit` of their own, only keep the holders.
pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    characters::init(cx);
    #[cfg(all(target_os = "macos", not(test)))]
    appkit::install(cx);
    #[cfg(not(target_os = "macos"))]
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

/// The person's keyboard in `window` is in `surface` now, because of `cause`.
pub fn set(window: &Window, surface: Surface, cause: Cause) {
    seam::set_in(known(window), surface, cause);
}

/// `window` shows the tab known by `tab` now, and hands it the keyboard: what that tab had
/// the keyboard in when it was last left, if it's known.
pub fn tab_shown(window: &Window, tab: u64) -> Option<Surface> {
    seam::tab_shown(known(window), tab)
}

/// GPUI's focus moved into the page in `pane`, the view whose focus is the page's.
pub fn entered_page(window: &Window, pane: PaneId) {
    set(window, Surface::Page(pane), Cause::Focused);
}

/// GPUI's focus left the page in `pane` for `part` of Koyo's own.
pub fn left_page(window: &Window, pane: PaneId, part: Part) {
    seam::left_page(known(window), pane, part);
}

/// `window` became the key window, or stopped being it; and GPUI's focus is where it is now
/// (`follow_focus`). Read as the window's key state changes and as each frame of the shell is
/// drawn, so a change the holders missed is made up.
pub fn key(window: &Window, cx: &mut App) {
    follow_focus(window, cx);
    let key = window.is_window_active();
    seam::key(known(window), key);
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
    let Some((_, Surface::Page(pane))) = seam::held(key) else {
        return;
    };
    let view = crate::browser::page_view(pane, cx);
    let Some(part) = (match &view {
        Some(view) => view.read(cx).part_with_focus(window, cx),
        None => Some(Part::Other),
    }) else {
        return;
    };
    seam::left_page(key, pane, part);
}

/// The page in `pane` closed.
pub fn page_closed(pane: PaneId) {
    seam::page_closed(pane);
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
        seam::dialog(key_of(over), true);
        DialogOver(over)
    }
}

impl Drop for DialogOver {
    fn drop(&mut self) {
        seam::dialog(key_of(self.0), false);
    }
}

/// Each window the keyboard is kept for, its holder, its first responder and whether it's key,
/// and whether the app is in front, for the native pages' harness (`browser/native/probe.rs`).
#[cfg(all(target_os = "macos", debug_assertions, not(test)))]
pub fn describe() -> String {
    appkit::describe()
}

/// What has the keyboard in `window`, as a test reads it.
#[cfg(test)]
pub fn holder(window: AnyWindowHandle) -> Option<Holder> {
    seam::holder(key_of(window))
}
