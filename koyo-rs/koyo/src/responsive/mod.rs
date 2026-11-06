//! Responsive mode's chrome (`docs/responsive.md` 2.3, 3.3), one for every pane that shows
//! something at a chosen size: a browser page at width × height pixels, a terminal at
//! cols × rows cells. The device bar over it (`bar`), the sunken area it's centred in, its
//! outline, and the grips on its edges that drag it to a size (`grips`). What differs between
//! panes is what they give it: the units, the presets, the limits of a side, and how the
//! content is fitted to the area (a page scaled, a terminal's font made smaller).

pub mod bar;
pub mod grips;

use gpui_kit::{BorderStyle, Bounds, Pixels, Size, Window, outline, point, px, size};
use koyo_app_control::ops::browser::{FitWord, ViewportZoom};

pub use grips::{Drag, Grip, Resizable};

use crate::theme::Palette;

/// The room round the content in its area, on each side.
pub const GUTTER: f32 = 16.;

/// How large the content shows in its area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    /// As large as fits, at most its true size.
    Fit,
    /// A percent of its true size.
    Percent(u32),
}

impl Zoom {
    /// The zoom menu's rows.
    pub const ALL: [Zoom; 4] = [
        Zoom::Fit,
        Zoom::Percent(50),
        Zoom::Percent(75),
        Zoom::Percent(100),
    ];

    /// What the zoom menu's button says, the content shown at `shown` percent: `Fit (83%)`.
    pub fn label(self, shown: u32) -> String {
        match self {
            Zoom::Fit => format!("Fit ({shown}%)"),
            Zoom::Percent(percent) => format!("{percent}%"),
        }
    }

    /// The zoom's row in the menu.
    pub fn row(self) -> String {
        match self {
            Zoom::Fit => "Fit".to_owned(),
            Zoom::Percent(percent) => format!("{percent}%"),
        }
    }
}

/// A zoom as `browser_viewport` names it.
impl From<Zoom> for ViewportZoom {
    fn from(zoom: Zoom) -> Self {
        match zoom {
            Zoom::Fit => ViewportZoom::Fit(FitWord::Fit),
            Zoom::Percent(percent) => ViewportZoom::Percent(percent),
        }
    }
}

/// A zoom `browser_viewport` names, whatever its percent: the menu's are checked apart.
impl From<ViewportZoom> for Zoom {
    fn from(zoom: ViewportZoom) -> Self {
        match zoom {
            ViewportZoom::Fit(_) => Zoom::Fit,
            ViewportZoom::Percent(percent) => Zoom::Percent(percent),
        }
    }
}

/// Where content `width` × `height` shown pixels sits in `area`: centred across it, at its
/// top, past the gutter; content wider than the area keeps its left edge in sight.
pub fn place(area: Bounds<Pixels>, width: f32, height: f32) -> Bounds<Pixels> {
    let shown = size(px(width), px(height));
    let left = ((area.size.width - shown.width) / 2.).max(px(GUTTER));
    Bounds::new(area.origin + point(left, px(GUTTER)), shown)
}

/// The room for content in an area of `size`, inside the gutters.
pub fn room(size: Size<Pixels>) -> Size<Pixels> {
    size.map(|side| (side - px(2. * GUTTER)).max(px(0.)))
}

/// The 1 px line round content shown at `shown`.
pub fn paint_outline(shown: Bounds<Pixels>, p: Palette, window: &mut Window) {
    window.paint_quad(outline(
        shown.dilate(px(1.)),
        p.hairline,
        BorderStyle::Solid,
    ));
}

#[cfg(test)]
mod tests;
