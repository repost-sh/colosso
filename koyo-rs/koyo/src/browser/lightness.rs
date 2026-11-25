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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui_kit::{AppContext as _, Bounds, TestAppContext, point, px, size};
    use koyo_browser_ipc::{HostEvent, Tone};

    use super::glass_dark;
    use crate::browser::BrowserView;
    use crate::browser::host::fake::FakeHost;
    use crate::test_support::{draw, in_window};

    /// Glass over a page takes the other tone than the page's, as the page says how light it
    /// is: dark over a white page, light over a black one; none while it hasn't said, or for
    /// glass over none of it.
    #[gpui_kit::test]
    fn glass_over_a_page_is_dark_over_a_light_one_and_light_over_a_dark_one(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let host = FakeHost::serve("glass-tone", cx);
        let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
            cx.new(|cx| BrowserView::new("https://a.example/".into(), Path::new("/work/app"), window, cx))
        });
        draw(window, cx);
        let pane = view.read_with(cx, |view, _| view.pane);
        let dark = |cx: &mut TestAppContext| {
            let launcher = view.read_with(cx, |view, _| {
                glass_dark([view], view.page_bounds().expect("the page is drawn"), None)
            });
            (launcher, view.update(cx, |view, _| view.read_bar_dark()))
        };
        let says = |lightness: f32, canvas: [u8; 3], cx: &mut TestAppContext| {
            let lightness = vec![lightness; Tone::ACROSS * Tone::DOWN];
            host.says(HostEvent::Tone { pane, tone: Tone { lightness, canvas } }, cx);
        };
        assert_eq!(dark(cx), (None, None), "the page hasn't said how light it is");
        says(1., [255; 3], cx);
        assert_eq!(dark(cx), (Some(true), Some(true)), "a launcher and the bar over a white page");
        says(0., [0; 3], cx);
        assert_eq!(dark(cx), (Some(false), Some(false)), "a launcher and the bar over a black page");

        let beyond = Bounds::new(point(px(-200.), px(-200.)), size(px(100.), px(100.)));
        let read = view.read_with(cx, |view, _| glass_dark([view], beyond, None));
        assert_eq!(read, None, "glass over none of the page");
    }
}
