//! A page full screen, as a video's button puts it (`requestFullscreen`). Chromium fills the
//! page's own view with the element; the app then gives the page the screen: the window goes
//! full screen, unless it is already, and shows that page alone, until the page leaves full
//! screen, closes, the person presses Escape, or takes the window out of full screen. With
//! Browser › "Full screen fills the screen" off, the page stays in its pane, as Chromium leaves
//! it.
//!
//! The page says where its full screen is; the shell, which draws the window, keeps what the
//! app did with the window for it ([`settle`]), so the window comes out of full screen however
//! the page left it.

use gpui_kit::{App, Context, Entity, Global, WeakEntity, Window};
use koyo_browser_ipc::AppRequest;

use super::view::BrowserView;
use crate::settings;

/// Where a page's full screen is.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Fullscreen {
    #[default]
    Off,
    /// In its pane, the window as it was.
    Pane,
    /// On the whole screen, which the window shows it alone on.
    Screen,
}

/// The page on the whole screen, and what the app did with the window for it.
#[derive(Default)]
enum OnScreen {
    #[default]
    None,
    /// A page went full screen: the window goes full screen as the shell next draws it.
    Asked(WeakEntity<BrowserView>),
    /// The window shows the page alone, full screen.
    Shown {
        page: WeakEntity<BrowserView>,
        /// Whether the app put the window in full screen for it, so takes it out again.
        took: bool,
        /// Whether the window has been seen full screen since: from then on, the window out of
        /// full screen means the person took it out.
        seen: bool,
    },
}

impl Global for OnScreen {}

/// `page`, while it's alive and full screen on the whole screen.
fn filling(page: &WeakEntity<BrowserView>, cx: &App) -> Option<Entity<BrowserView>> {
    (page.upgrade()).filter(|page| page.read(cx).fullscreen == Fullscreen::Screen)
}

/// The page full screen on the whole screen, if one is, which the window then shows alone, as
/// the shell draws it: the window goes full screen for the page as it starts, focused on it,
/// and comes out once the page has left, closed or not, if the app put it in.
pub fn settle(window: &mut Window, cx: &mut App) -> Option<Entity<BrowserView>> {
    let (now, page) = match std::mem::take(cx.default_global::<OnScreen>()) {
        OnScreen::None => (OnScreen::None, None),
        OnScreen::Asked(asked) => match filling(&asked, cx) {
            Some(page) => {
                let took = !window.is_fullscreen();
                if took {
                    window.toggle_fullscreen();
                }
                let focus = page.read(cx).focus.clone();
                window.focus(&focus, cx);
                let seen = !took;
                (
                    OnScreen::Shown {
                        page: asked,
                        took,
                        seen,
                    },
                    Some(page),
                )
            }
            None => (OnScreen::None, None),
        },
        OnScreen::Shown { page, took, seen } => {
            if let Some(shown) = filling(&page, cx) {
                (OnScreen::Shown { page, took, seen }, Some(shown))
            } else {
                if took && window.is_fullscreen() {
                    window.toggle_fullscreen();
                }
                (OnScreen::None, None)
            }
        }
    };
    cx.set_global(now);
    page
}

/// The window changed size: once it was full screen for the page, it out of full screen means
/// the person took it out, and the page leaves full screen too.
pub fn window_resized(window: &mut Window, cx: &mut App) {
    let full = window.is_fullscreen();
    let OnScreen::Shown { page, seen, .. } = cx.default_global::<OnScreen>() else {
        return;
    };
    if full {
        *seen = true;
        return;
    }
    if !std::mem::take(seen) {
        return;
    }
    if let Some(page) = page.upgrade() {
        page.update(cx, BrowserView::exit_fullscreen);
    }
}

impl BrowserView {
    /// The page went full screen (`on`) or left it.
    pub(super) fn fullscreen_changed(&mut self, on: bool, cx: &mut Context<Self>) {
        self.fullscreen = if !on {
            Fullscreen::Off
        } else if settings::get(cx).fullscreen_fills_screen {
            let me = cx.entity().downgrade();
            cx.set_global(OnScreen::Asked(me));
            Fullscreen::Screen
        } else {
            Fullscreen::Pane
        };
        cx.notify();
        cx.refresh_windows();
    }

    /// Has the page leave full screen, if it's in it: whether it was. It says so as it does
    /// (`fullscreen_changed`).
    pub(super) fn exit_fullscreen(&mut self, cx: &mut Context<Self>) -> bool {
        let on = self.is_fullscreen();
        if on {
            let pane = self.pane;
            self.send_to_page(AppRequest::ExitFullscreen { pane }, cx);
        }
        on
    }

    /// Whether the page is full screen, in its pane or on the whole screen.
    pub(super) fn is_fullscreen(&self) -> bool {
        self.fullscreen != Fullscreen::Off
    }

    /// Whether the page is full screen on the whole screen, which it then fills alone.
    pub(super) fn fills_screen(&self) -> bool {
        self.fullscreen == Fullscreen::Screen
    }
}
