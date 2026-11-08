//! The sidebar folded away over a page and brought back, saved as it moves: a dark page,
//! so a strip of anything else between the sidebar and the page, or at the page's far edge,
//! would show. Each picture's row through the page's middle is looked at from the page's left
//! edge to its right one, and has to be the page's own dark all the way.

use std::time::Duration;

use gpui_kit::Entity;
use image::RgbaImage;

use super::agent::next_frame;
use super::{Check, loaded, picture};
use crate::browser::BrowserView;
use crate::visual::Result;

/// A page dark from edge to edge, its words in its top left.
pub(super) const FOLD: &str = "<!doctype html><title>Fold</title>\
    <body style='margin:0;height:100vh;background:#111;color:#ddd;\
    font:16px -apple-system,sans-serif'><p style='margin:24px'>Dark to its edges</p>";

/// When, after ⌘B, each picture of the fold is taken, on the check's clock, which the rails'
/// spring moves by: as it starts, through the 120 ms it moves for, and once it's over.
const SHOTS: [u64; 5] = [16, 40, 70, 100, 400];

/// How light a pixel of the page's row may be: the page is `#111`.
const DARK: u8 = 48;

impl Check {
    /// A third tab on the dark page, the sidebar folded over it and brought back.
    pub(super) fn fold(&mut self, port: u16) -> Result<()> {
        let page = self.open_served(port, "/fold", "the dark page", |view| loaded(view, "Fold"))?;
        for way in ["fold", "unfold"] {
            self.keys("cmd-b");
            let mut elapsed = 0;
            for (index, after) in SHOTS.into_iter().enumerate() {
                (self.drawn.cx).advance_clock(Duration::from_millis(after - elapsed));
                elapsed = after;
                next_frame(&mut self.drawn)?;
                let shot = picture(&mut self.drawn)?;
                let name = format!("{way}-{index}.png");
                dark_across(&shot, &page, &mut self.drawn, &name)?;
                self.keep(&name, &shot)?;
            }
        }
        Ok(())
    }
}

/// Whether `shot`'s row through `page`'s middle is dark from the page's left edge to its
/// right one, where the window drew it as `shot` was taken.
fn dark_across(
    shot: &RgbaImage,
    page: &Entity<BrowserView>,
    drawn: &mut crate::visual::Drawn,
    name: &str,
) -> Result<()> {
    let window = drawn.window;
    let (origin, viewport, scale) = (drawn.cx)
        .update_window(window, |_, window, cx| {
            let view = page.read(cx);
            (view.origin, view.viewport, window.scale_factor())
        })
        .map_err(|error| error.to_string())?;
    let (wide, tall) = viewport.map_or((0, 0), |viewport| (viewport.width, viewport.height));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a page's place in a picture of its window"
    )]
    let pixel = |points: f32| (points * scale).round().max(0.) as u32;
    #[expect(clippy::cast_precision_loss, reason = "a page's size in points")]
    let (left, right, y) = (
        pixel(origin.x.as_f32()) + 1,
        pixel(origin.x.as_f32() + wide as f32).saturating_sub(2),
        pixel(origin.y.as_f32() + tall as f32 / 2.),
    );
    for x in left..=right.min(shot.width().saturating_sub(1)) {
        let [r, g, b, _] = shot.get_pixel(x, y.min(shot.height() - 1)).0;
        if r.max(g).max(b) > DARK {
            return Err(format!(
                "{name}: the page's row is ({r}, {g}, {b}) at x {x} of {left} to {right}, y {y}: \
                 not the page's dark"
            ));
        }
    }
    Ok(())
}
