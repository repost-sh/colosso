//! Presses the window never sees the end of. From a press on the page's native view until
//! the press is let go, the window keeps the pointer on the page (gpui-fast's native
//! capture): every press, move and release in the window goes to the page, and nothing
//! else in the window hears them. A release that never reaches the window would keep it so
//! for good, every tab and button dead until the app is started again. None comes for a
//! swipe, which `AppKit`'s event mapping hands on as a Navigate press with no release; one
//! that something else takes, as a menu's tracking does, is the same to the window.
//!
//! The page is handed every press the window holds on it, and every release that ends one,
//! so it knows the presses the window holds (`window_presses`). The pointer moving with no
//! button held while there are any belies them: the window is then had to let them go
//! (`let_go`).

use koyo_browser_ipc::MouseKind;
use gpui_kit::{Context, MouseUpEvent, Pixels, PlatformInput, Point, Window};

use crate::browser::view::BrowserView;

impl BrowserView {
    /// Ends the press the page has, if any, at `at`, and has the window let go of the
    /// presses it holds on the page: a release of each goes through it, at `at`, once the
    /// handler at hand is done.
    pub(in crate::browser) fn let_go(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = window.modifiers();
        if let Some(button) = self.pressed.take() {
            self.mouse(MouseKind::Up, at, Some(button), 1, modifiers, cx);
        }
        let held = std::mem::take(&mut self.window_presses);
        if held.is_empty() {
            return;
        }
        window.defer(cx, move |window, cx| {
            for button in held {
                let release = MouseUpEvent {
                    button,
                    position: at,
                    modifiers,
                    click_count: 1,
                };
                window.dispatch_event(PlatformInput::MouseUp(release), cx);
            }
        });
    }
}

#[cfg(test)]
mod tests;
