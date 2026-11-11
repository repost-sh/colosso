//! The picture in a message, clicked where it shows, open in the viewer over the chat, and closed
//! with Esc.

use std::time::Instant;

use gpui_kit::{Modifiers, Pixels, Point, point, px};
use koyo_agent::protocol::model::Prompt;

use super::{Check, PARSED_IN, PATIENCE};
use crate::chat::attachments::{self, Source};
use crate::visual::{Result, prompt};

/// The picture's colour, which finds it on screen.
const MAGENTA: [u8; 3] = [240, 0, 240];

impl Check {
    /// A message with a picture, the picture clicked where it shows, the viewer over the chat, then
    /// Esc.
    pub(super) fn picture(&mut self) -> Result<()> {
        let folder = attachments::folder().ok_or("there's no attachments folder")?;
        let picture = image::RgbImage::from_pixel(320, 200, image::Rgb(MAGENTA));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(picture)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(|error| error.to_string())?;
        let kept = attachments::keep(Source::Picture(bytes), &folder)?;
        let id = self.chat(Prompt {
            attachments: vec![kept],
            ..prompt("Here's the picture.")
        })?;
        let view = self.view(&id)?;
        let at = self.magenta()?;
        let window = self.drawn.window;
        self.drawn
            .cx
            .simulate_click(window, at, Modifiers::default());
        self.drawn.draw()?;
        if self.drawn.cx.read(|cx| view.read(cx).picture.is_none()) {
            return Err(format!("a click at {at:?} opened no picture"));
        }
        let start = Instant::now();
        self.drawn.until(PATIENCE, |drawn| {
            let drawn_yet = drawn.cx.read(|cx| {
                view.read(cx)
                    .picture
                    .as_ref()
                    .is_some_and(|viewer| viewer.read(cx).is_drawn())
            });
            Ok(drawn_yet && start.elapsed() >= PARSED_IN)
        })?;
        self.both("picture-open")?;
        self.drawn.cx.simulate_keystrokes(window, "escape");
        self.drawn.draw()?;
        if self.drawn.cx.read(|cx| view.read(cx).picture.is_some()) {
            return Err("Esc left the picture open".into());
        }
        self.both("picture-closed")
    }

    /// Where the picture shows in the window, found by its colour: the middle of its pixels.
    fn magenta(&mut self) -> Result<Point<Pixels>> {
        self.drawn.draw()?;
        let window = self.drawn.window;
        let image =
            (self.drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())?;
        let scale = self
            .drawn
            .cx
            .update_window(window, |_, window, _| window.scale_factor())
            .map_err(|error| error.to_string())?;
        let found: Vec<(u32, u32)> = (image.enumerate_pixels())
            .filter(|(_, _, pixel)| {
                let [r, g, b, _] = pixel.0;
                r.abs_diff(MAGENTA[0]) < 12 && g < 24 && b.abs_diff(MAGENTA[2]) < 12
            })
            .map(|(x, y, _)| (x, y))
            .collect();
        let count = u32::try_from(found.len()).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("the picture isn't on screen".into());
        }
        let (x, y) = found.iter().fold((0_u64, 0_u64), |(x, y), (px_x, px_y)| {
            (x + u64::from(*px_x), y + u64::from(*px_y))
        });
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window's pixels are far below 2^24"
        )]
        let middle = (
            x as f32 / f64::from(count) as f32 / scale,
            y as f32 / f64::from(count) as f32 / scale,
        );
        Ok(point(px(middle.0), px(middle.1)))
    }
}
