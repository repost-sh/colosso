//! Native pages: a page as a Chrome-style browser whose own views are shown in Koyo's window,
//! Chromium presenting its frames itself and `AppKit` handing it its input
//! (`docs/native-pages.md`).
//!
//! The browser host runs in Koyo's own process for it (`koyo_browser_host::embedded`): an
//! `NSView` can't be shown in another process's window. The app talks to it over the same
//! socket as to a host process, so everything but a page's picture and its pointer and keys
//! is as it is off screen. This module is the app's side of what differs: whether pages are
//! native, how the host is started in the app, and a page's view in its pane
//! ([`NativePage`]), a native surface of GPUI's composition between the window's content and
//! its overlays.
//!
//! Chosen once, as Koyo starts, for all its pages ([`decide`]): by `KOYO_NATIVE_PAGES` (`1` or
//! `0`) where it's set, else by the setting (`native_pages`). Koyo has to run from a bundle
//! that holds the engine; from a bare binary, pages stay off screen.

use std::path::PathBuf;
use std::sync::OnceLock;

use gpui_kit::{App, Bounds, Pixels, Window};
use koyo_browser_ipc::PaneId;
use koyo_browser_ipc::client::{Embedded, Launch};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::native::surface::{Hosted, Surface};

#[cfg(debug_assertions)]
pub mod probe;

/// The engine native pages run on, in the bundle Koyo runs from; none where pages are off
/// screen. Unset until [`decide`].
static ENGINE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Settles whether this run's pages are native, before the engine first starts: asked for,
/// and Koyo runs from a bundle that holds the engine.
pub fn decide(cx: &App) {
    ENGINE.get_or_init(|| {
        let asked = match std::env::var("KOYO_NATIVE_PAGES") {
            Ok(value) => value != "0",
            Err(_) => crate::settings::get(cx).native_pages,
        };
        if !asked {
            return None;
        }
        let engine = super::engine::bundled();
        if engine.is_none() {
            log::warn!(
                "native pages need Koyo to run from a bundle that holds the browser engine \
                 (script/dev-bundle); pages stay off screen"
            );
        }
        engine
    });
}

/// Whether this run's pages are native.
pub fn enabled() -> bool {
    ENGINE.get().is_some_and(Option::is_some)
}

/// How the host is started in Koyo's process for `launch`, where pages are native.
pub fn embedded(launch: &Launch) -> Option<Embedded> {
    let engine = ENGINE.get()?.clone()?;
    let profiles = launch.profiles.clone();
    Some(Embedded {
        start: Box::new(move |stream, token| {
            koyo_browser_host::embedded::start(koyo_browser_host::embedded::Start {
                stream,
                token,
                profiles,
                engine,
            });
        }),
        finish: Box::new(koyo_browser_host::embedded::finish),
    })
}

/// A page's view in its pane: a view of Koyo's in a native surface, which the page's own
/// views fill for as long as the pane draws it.
pub struct NativePage {
    pane: PaneId,
    container: Retained<NSView>,
    surface: Surface,
    /// Whether the page has been placed yet.
    placed: std::cell::Cell<bool>,
}

impl Hosted for NativePage {
    fn into_surface(self) -> Surface {
        koyo_browser_host::native::release(self.pane, &self.container);
        self.surface
    }
}

impl NativePage {
    /// A place for `pane`'s page in `window`.
    pub fn new(window: &Window, pane: PaneId) -> Result<NativePage, String> {
        let main = MainThreadMarker::new().ok_or("not on the main thread")?;
        let container = NSView::new(main);
        container.setWantsLayer(true);
        let surface = Surface::new(window, &container)?;
        Ok(NativePage {
            pane,
            container,
            surface,
            placed: std::cell::Cell::new(false),
        })
    }

    /// Puts the page over `bounds`, as much as `clip` shows, and its holder over it on screen;
    /// whether that's the first time.
    pub fn place(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, window: &Window) -> bool {
        self.surface.place(bounds, clip, window);
        // The view GPUI draws in is the page's home: the keyboard's when the page hasn't it.
        let Some(home) = crate::native::window::gpui_view(window) else {
            return false;
        };
        let Some(ns_window) = home.window() else {
            return false;
        };
        let height = home.frame().size.height;
        let points = |pixels: Pixels| f64::from(f32::from(pixels));
        let (x, y) = (points(bounds.left()), points(bounds.top()));
        let (w, h) = (points(bounds.size.width), points(bounds.size.height));
        let in_window = home.convertRect_toView(
            NSRect::new(NSPoint::new(x, height - y - h), NSSize::new(w, h)),
            None,
        );
        let screen = ns_window.convertRectToScreen(in_window);
        koyo_browser_host::native::place(self.pane, &self.container, &home, screen);
        !self.placed.replace(true)
    }
}
