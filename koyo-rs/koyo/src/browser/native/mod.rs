//! A page in Koyo's window: a Chrome-style browser whose own views are shown in the pane,
//! Chromium presenting its frames itself and `AppKit` handing it its input
//! (`docs/native-pages.md`).
//!
//! The browser host runs in Koyo's own process for it (`koyo_browser_host::embedded`): an
//! `NSView` can't be shown in another process's window. The app talks to it over a socket, as
//! it would to a process of its own. This module is the app's side of it: how the host is
//! started in the app ([`start`]), and a page's view in its pane ([`NativePage`]), a native
//! surface of GPUI's composition between the window's content and its overlays.

use std::io;

use gpui_kit::{Bounds, Pixels, Window};
use koyo_browser_ipc::PaneId;
use koyo_browser_ipc::client::{Connection, Embedded, Launch, Sink};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::native::surface::{Hosted, Surface};

#[cfg(debug_assertions)]
pub mod probe;

/// Has a debug build say so in its log, with where from, whenever the app is forked from now
/// on. It holds the engine from here, and a fork of it can wait for good on a lock one of
/// the engine's threads held (`docs/native-pages.md` 11). The lint turns down the calls that
/// always fork (`clippy.toml`); this shows what it can't see: a program named, not given by
/// its path, with a `PATH` of its own to look in, and a fork a crate makes itself.
#[cfg(debug_assertions)]
fn say_when_forked() {
    extern "C" fn forking() {
        let from = std::backtrace::Backtrace::force_capture();
        log::error!("Koyo is being forked, which can hang with the browser engine in it:\n{from}");
    }
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: the handler logs, on the thread that forks, before the fork is made.
    ONCE.call_once(|| unsafe {
        libc::pthread_atfork(Some(forking), None, None);
    });
}

/// Starts the browser host in Koyo's own process, on the engine whose host `launch` names, and
/// completes the handshake; from then on events go to `sink`. The handshake waits for the host,
/// which starts on the main thread: call this off it.
pub fn start(launch: &Launch, sink: impl Sink) -> io::Result<Connection> {
    #[cfg(debug_assertions)]
    say_when_forked();
    let engine = (launch.executable.ancestors().nth(3))
        .ok_or_else(|| io::Error::other("the browser engine isn't in an app of its own"))?
        .to_path_buf();
    let profiles = launch.profiles.clone();
    let embedded = Embedded {
        start: Box::new(move |stream, token, clock| {
            koyo_browser_host::embedded::start(koyo_browser_host::embedded::Start {
                stream,
                token,
                clock,
                profiles,
                engine,
            });
        }),
        finish: Box::new(koyo_browser_host::embedded::finish),
    };
    Connection::start_embedded(launch, embedded, sink)
}

/// A page's view in its pane: a view of Koyo's in a native surface, which the page's own
/// views fill for as long as the pane draws it.
pub struct NativePage {
    pane: PaneId,
    container: Retained<NSView>,
    surface: Surface,
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
            size: std::cell::Cell::new(None),
            resized: std::cell::Cell::new(None),
        })
    }

    /// Puts the page over `bounds`, as much as `clip` shows, rounding the corners `rounded` says,
    /// and its holder over it on screen.
    ///
    /// On whole points, whatever the pane's are: a page is a whole number of points in size,
    /// as its holder, a window, and Chromium's own layout are. In a view half a point
    /// over, the layer Chromium's picture is in is rounded up each time the view is resized
    /// and never comes back down, so it ends up taller than the view by what the halves add
    /// up to, the picture's top cut off and a gap at its foot (`docs/native-pages.md` 6). A
    /// pane's edges fall between points whenever a pointer set them: on a screen of two
    /// pixels a point, a pointer is on half points. Where the page reaches the edge of what
    /// shows it, it goes out to the next whole point and `clip` trims it, so nothing behind
    /// it shows beside it.
    pub fn place(
        &self,
        bounds: Bounds<Pixels>,
        clip: Bounds<Pixels>,
        rounded: gpui_kit::Corners<bool>,
        window: &Window,
    ) {
        let bounds = crate::native::surface::on_whole_points(bounds, clip);
        self.surface.place(bounds, clip, window);
        self.surface.round(rounded, f64::from(crate::theme::CARD_RADIUS));
        if self.size.replace(Some(bounds.size)).is_some_and(|was| was != bounds.size) {
            self.resized.set(Some(std::time::Instant::now()));
        }
        let Some(home) = crate::native::window::gpui_view(window) else {
            return;
        };
        let Some(ns_window) = home.window() else {
            return;
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
    }

    /// Whether the page's size has just changed, as while an edge of its pane is dragged.
    pub fn resizing(&self) -> bool {
        self.resized.get().is_some_and(|at| at.elapsed() < RESIZING_FOR)
    }
}
