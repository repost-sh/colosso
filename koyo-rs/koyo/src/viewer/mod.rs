//! Files shown as what they are rather than as their text, in the same tab a file's text is:
//! pictures, SVG drawings and PDF documents. Each is read and drawn on a thread of its own,
//! at no more pixels than the screen shows: a picture decoded at the size it's shown, a
//! drawing or a page drawn in tiles only where it's in sight, at the zoom it's shown at. A
//! trackpad's pinch, or ⌘ and the wheel, zooms about the pointer; scrolling or dragging moves
//! about; ⌘=, ⌘- and ⌘0 zoom in, out and back to fit.
//!
//! Pictures are read with the system's decoders (`ImageIO`, Preview's), drawings with resvg
//! (GPUI's own) and documents with Quartz (Preview's): nothing the app doesn't link already,
//! and every format Preview opens.

mod bitmap;
mod canvas;
mod element;
mod input;
mod lane;
mod pdf;
mod picture;
mod plan;
mod render;
pub(crate) use render::size_on_disk;
mod svg;
mod tiles;
mod view;

#[cfg(test)]
mod tab_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use gpui_kit::{App, KeyBinding};

pub use view::Viewer;

use canvas::Fit;

/// What a viewer shows a file as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Picture,
    Svg,
    Pdf,
}

impl Kind {
    /// How a file of this kind fits its pane until it's zoomed: a picture whole but no larger
    /// than its own size, a drawing whole at any size, a document's pages as wide as the pane.
    fn fit(self) -> Fit {
        match self {
            Kind::Picture => Fit::Whole { most: Some(1.) },
            Kind::Svg => Fit::Whole { most: None },
            Kind::Pdf => Fit::Width,
        }
    }
}

/// What the file at `path` is shown as, by its extension: `None` for a file shown as text.
/// SVG is text too, which its tab shows on asking.
pub fn kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "png" | "apng" | "jpg" | "jpeg" | "jpe" | "jfif" | "pjpeg" | "gif" | "webp" | "heic"
        | "heif" | "avif" | "jxl" | "tif" | "tiff" | "bmp" | "ico" | "cur" | "icns" | "tga"
        | "psd" | "exr" | "jp2" | "dds" => Kind::Picture,
        "svg" | "svgz" => Kind::Svg,
        "pdf" => Kind::Pdf,
        _ => return None,
    })
}

/// Whether the file at `path`, shown as what it is, is text too, which its tab shows on
/// asking: an SVG's source, though not a gzipped one's.
pub fn has_text(path: &Path) -> bool {
    (path.extension()).is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

/// The key context a viewer takes, which its keys are bound in.
const CONTEXT: &str = "Viewer";

/// Binds a viewer's keys: zooming in, out and to fit as a browser's are, and moving about
/// with the arrows, the page keys and the space bar.
pub fn init(cx: &mut App) {
    use input::{
        ActualSize, End, Home, PageDown, PageUp, ScrollDown, ScrollLeft, ScrollRight, ScrollUp,
        ZoomIn, ZoomOut, ZoomToFit,
    };
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("cmd-=", ZoomIn, context),
        KeyBinding::new("cmd-+", ZoomIn, context),
        KeyBinding::new("cmd--", ZoomOut, context),
        KeyBinding::new("cmd-0", ZoomToFit, context),
        KeyBinding::new("cmd-shift-0", ActualSize, context),
        KeyBinding::new("up", ScrollUp, context),
        KeyBinding::new("down", ScrollDown, context),
        KeyBinding::new("left", ScrollLeft, context),
        KeyBinding::new("right", ScrollRight, context),
        KeyBinding::new("pageup", PageUp, context),
        KeyBinding::new("pagedown", PageDown, context),
        KeyBinding::new("space", PageDown, context),
        KeyBinding::new("shift-space", PageUp, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
    ]);
}
