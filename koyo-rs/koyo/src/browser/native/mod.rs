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
    /// The size it was last placed at, and when that last changed.
    size: std::cell::Cell<Option<gpui_kit::Size<Pixels>>>,
    resized: std::cell::Cell<Option<std::time::Instant>>,
}

/// How long after its size last changed a page counts as being resized: a drag's moves come
/// far closer together than this.
const RESIZING_FOR: std::time::Duration = std::time::Duration::from_millis(250);

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
            size: std::cell::Cell::new(None),
            resized: std::cell::Cell::new(None),
        })
    }

    /// Puts the page over `bounds`, as much as `clip` shows, and its holder over it on screen;
    /// whether that's the first time.
    ///
    /// On whole points, whatever the pane's are: a native page is a whole number of points in
    /// size, as its holder, a window, and Chromium's own layout are. In a view half a point
    /// over, the layer Chromium's picture is in is rounded up each time the view is resized
    /// and never comes back down, so it ends up taller than the view by what the halves add
    /// up to, the picture's top cut off and a gap at its foot (`docs/native-pages.md` 6). A
    /// pane's edges fall between points whenever a pointer set them: on a screen of two
    /// pixels a point, a pointer is on half points.
    pub fn place(&self, bounds: Bounds<Pixels>, clip: Bounds<Pixels>, window: &Window) -> bool {
        let bounds = crate::native::surface::on_whole_points(bounds);
        self.surface.place(bounds, clip, window);
        if self.size.replace(Some(bounds.size)).is_some_and(|was| was != bounds.size) {
            self.resized.set(Some(std::time::Instant::now()));
        }
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

    /// Whether the page's size has just changed, as while an edge of its pane is dragged.
    pub fn resizing(&self) -> bool {
        self.resized.get().is_some_and(|at| at.elapsed() < RESIZING_FOR)
    }
}
