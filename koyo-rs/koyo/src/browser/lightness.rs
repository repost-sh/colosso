//! How light the pages under glass over them are, which then takes the other tone
//! (`theme::glass`): the mean luminance of each page's latest frame there, read from the surface
//! the host shares with the app. A grid of its pixels is enough, as glass blurs what's under it and
//! the page is read as a whole there.

use gpui_kit::{Bounds, Pixels};

use super::frames::Frame;
use super::view::BrowserView;
use crate::theme::glass;

/// How many pixels across and down a region are read.
const ACROSS: usize = 24;
const DOWN: usize = 8;
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
    /// that is; none where it isn't drawn, has no frame yet, or isn't under it.
    pub(super) fn under(&self, region: Bounds<Pixels>) -> Option<Under> {
        let frame = self.frame.as_ref().filter(|_| self.shown.is_some())?;
        let over = self.page_bounds()?.intersect(&region);
        let (width, height) = (over.size.width.as_f32(), over.size.height.as_f32());
        if width <= 0. || height <= 0. {
            return None;
        }
        let (from, to) = (
            self.page_point(over.origin),
            self.page_point(over.bottom_right()),
        );
        // From the page's points to its frame's device pixels.
        let device = |at: Pixels| at.as_f32() * frame.scale;
        let area = [device(from.x), device(from.y), device(to.x), device(to.y)];
        Some(Under {
            area: width * height,
            lightness: frame.lightness(area)?,
        })
    }
}

impl Frame {
    /// The mean luminance of the page within `[left, top, right, bottom]`, in its device pixels;
    /// none where that's none of the page, or the surface can't be read.
    fn lightness(&self, [left, top, right, bottom]: [f32; 4]) -> Option<f32> {
        let (width, height) = (self.size.width as usize, self.size.height as usize);
        let clamp = |at: f32, end: usize| (at.max(0.) as usize).min(end);
        let (left, right) = (clamp(left, width), clamp(right, width));
        let (top, bottom) = (clamp(top, height), clamp(bottom, height));
        if right <= left || bottom <= top {
            return None;
        }
        // The middle of each cell of an `ACROSS` by `DOWN` grid over the region.
        let at = |from: usize, to: usize, cell: usize, cells: usize| {
            from + (to - from) * (2 * cell + 1) / (2 * cells)
        };
        let sum = self.with_pixels(|pixel| {
            let cells = (0..DOWN).flat_map(|down| (0..ACROSS).map(move |across| (across, down)));
            cells
                .map(|(across, down)| {
                    let x = at(left, right, across, ACROSS);
                    luminance(pixel(x, at(top, bottom, down, DOWN)))
                })
                .sum::<f32>()
        })?;
        Some(sum / (ACROSS * DOWN) as f32)
    }
}

/// The relative luminance of a pixel, in BGRA with its alpha premultiplied, as it shows over the
/// white a page is drawn on.
fn luminance([blue, green, red, alpha]: [u8; 4]) -> f32 {
    let white = 1. - f32::from(alpha) / 255.;
    let linear = |channel: u8| {
        let shown = (f32::from(channel) / 255. + white).min(1.);
        if shown <= 0.04045 {
            shown / 12.92
        } else {
            ((shown + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
}

#[cfg(test)]
mod tests {
    use super::luminance;

    #[test]
    fn luminance_runs_from_black_to_white() {
        assert!(luminance([0, 0, 0, 255]) < 0.001);
        assert!(luminance([255, 255, 255, 255]) > 0.999);
        // sRGB's middle grey is about a fifth as luminous as white.
        let grey = luminance([119, 119, 119, 255]);
        assert!((0.17..0.2).contains(&grey), "{grey}");
    }

    #[test]
    fn green_is_lighter_than_blue() {
        assert!(luminance([0, 255, 0, 255]) > luminance([255, 0, 0, 255]));
    }

    #[test]
    fn a_clear_pixel_shows_the_white_under_it() {
        assert!(luminance([0, 0, 0, 0]) > 0.999);
    }
}
