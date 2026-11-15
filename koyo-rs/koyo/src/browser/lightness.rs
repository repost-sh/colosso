//! How light the pages under glass over them are, which then takes the other tone
//! (`theme::glass`): each page's lightness there, as the page last said it (`tone.rs`).
//! Chromium draws a page in layers of its own, which the app can't read, so the page is asked;
//! a grid of its points is enough, as glass blurs what's under it and the page is read as a
//! whole there.

use gpui_kit::{Bounds, Pixels};

use super::view::BrowserView;
use crate::theme::glass;

/// How much of glass pages have to be under for it to take its tone from them.
const MOSTLY: f32 = 0.5;

/// Whether glass over `region` of the window is dark, from `pages`, those drawn in the window,
/// where they're under it, given whether it `was`; none where they're under less than `MOSTLY` of
/// it, and its tone is the theme's.
pub fn glass_dark<'a>(
    pages: impl IntoIterator<Item = &'a BrowserView>,
    region: Bounds<Pixels>,
    was: Option<bool>,
) -> Option<bool> {
    let (mut area, mut light) = (0., 0.);
    for under in pages.into_iter().filter_map(|page| page.under(region)) {
        area += under.area;
        light += under.area * under.lightness;
    }
    let whole = region.size.width.as_f32() * region.size.height.as_f32();
    (area > 0. && area >= whole * MOSTLY).then(|| glass::dark_over(light / area, was))
}

/// What of a region of the window a page covers, and how light it is there.
#[derive(Clone, Copy, Debug)]
pub(super) struct Under {
    /// In square points.
    pub area: f32,
    /// Its mean relative luminance, from 0, black, to 1, white.
    pub lightness: f32,
}

impl BrowserView {
    /// How light the page is where it's under `region` of the window, and how much of `region`
    /// that is; none where it isn't drawn, hasn't said how light it is yet, or isn't under it.
    pub(super) fn under(&self, region: Bounds<Pixels>) -> Option<Under> {
        self.shown.as_ref()?;
        let over = self.page_bounds()?.intersect(&region);
        let (width, height) = (over.size.width.as_f32(), over.size.height.as_f32());
        if width <= 0. || height <= 0. {
            return None;
        }
        let (from, to) = (
            self.page_point(over.origin),
            self.page_point(over.bottom_right()),
        );
        let page = self.viewport?;
        let (wide, tall) = (page.width.max(1) as f32, page.height.max(1) as f32);
        let shares = [
            from.x.as_f32() / wide,
            from.y.as_f32() / tall,
            to.x.as_f32() / wide,
            to.y.as_f32() / tall,
        ];
        Some(Under {
            area: width * height,
            lightness: self.tone.as_ref()?.lightness(shares)?,
        })
    }
}
