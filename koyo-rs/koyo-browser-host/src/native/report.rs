//! What the app's harness reads of the native pages: where each one's views are, and whether
//! a page shown in a pane sits in it as it's meant to.

use std::fmt::Write as _;

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSView;
use objc2_foundation::NSRect;

use super::{Entry, KEYS, PAGES, class_of, find_view};

/// Every native page and where its views are.
#[must_use]
pub fn describe() -> String {
    PAGES.with(|pages| {
        let mut text = String::new();
        for (pane, entry) in pages.borrow().iter() {
            let place = entry.views.as_ref().map_or("no views yet".to_owned(), |(_, web)| {
                web.window().map_or("views in no window".to_owned(), |window| {
                    let class = class_of(&window).name().to_string_lossy();
                    format!("views in {class} visible {}", window.isVisible())
                })
            });
            let _ = write!(
                text,
                "\npage {pane}: {place}, drawn by a pane {}, on screen for the engine {}{}",
                entry.shown_in.is_some(),
                entry.on_screen,
                fit(entry)
            );
        }
        text
    })
}

/// How a page shown in a pane sits in it: its holder's size, the size of the app's view it's
/// in, and where its views are: the compositor's and the web contents' in the app's view, in
/// the web contents' the one the page is sized by, and under the compositor's the layer
/// Chromium's picture is in. `FITS` where each fills what it's in and the holder is that
/// size, which is how a page is meant to sit; `OFF` else.
fn fit(entry: &Entry) -> String {
    let (true, Some((container, _)), Some((compositor, web)), Some((_, holder))) =
        (entry.on_screen, &entry.shown_in, &entry.views, &entry.holder)
    else {
        return String::new();
    };
    let (bounds, holds) = (container.bounds(), holder.frame().size);
    let sized = find_view(web, KEYS).map(|view| view.frame());
    let picture = picture(compositor);
    let fits = compositor.frame() == bounds
        && web.frame() == bounds
        && holds == bounds.size
        && sized.is_none_or(|frame| frame == web.bounds())
        && picture.is_none_or(|(root, content)| root.size == bounds.size && content == root);
    let rect = |frame: NSRect| {
        let (at, size) = (frame.origin, frame.size);
        format!("{},{} {}x{}", at.x, at.y, size.width, size.height)
    };
    let or_none = |frame: Option<NSRect>| frame.map_or("none".to_owned(), rect);
    format!(
        "; {} holder {}x{} pane {}x{} compositor {} web {} sized {} layer {} picture {}",
        if fits { "FITS" } else { "OFF" },
        holds.width,
        holds.height,
        bounds.size.width,
        bounds.size.height,
        rect(compositor.frame()),
        rect(web.frame()),
        or_none(sized),
        or_none(picture.map(|(root, _)| root)),
        or_none(picture.map(|(_, content)| content))
    )
}

/// The compositor view's own layer and the layer in it that Chromium's picture is in, each
/// as its frame in the one above: the picture fills the view where the second is the first's
/// size, at its origin.
fn picture(compositor: &NSView) -> Option<(NSRect, NSRect)> {
    // SAFETY: a view's layer and a layer's sublayers and frame, read on the main thread.
    unsafe {
        let root: *mut AnyObject = msg_send![compositor, layer];
        if root.is_null() {
            return None;
        }
        let below: *mut AnyObject = msg_send![root, sublayers];
        let content: *mut AnyObject = msg_send![below, firstObject];
        if content.is_null() {
            return None;
        }
        let bounds: NSRect = msg_send![root, bounds];
        let frame: NSRect = msg_send![content, frame];
        Some((bounds, frame))
    }
}
