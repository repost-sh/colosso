//! Making a screen's picture, off the main thread: the wallpaper decoded by the system's
//! decoders at no more pixels than its layout needs, drawn into a bitmap the screen's size as
//! macOS lays it out, and compressed to ASTC, the only copy kept. The bitmap is dropped once
//! the blocks are out of it. How the picture looks, its dither, opacity and fade, is the
//! renderer's, as it's drawn (`texture::Look`).

use std::time::{Duration, Instant};

use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGContext, CGImage};

use super::astc::{self, Astc};
use super::layout;
use super::wallpaper::Wallpaper;
use crate::settings::Quality;
use crate::viewer::bitmap::Canvas;
use crate::viewer::picture;

/// What a screen's picture is made from: when any of it changes, the picture is made again.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub wallpaper: Wallpaper,
    /// The screen's size in points, and how many pixels make a point on it.
    pub screen: (f64, f64),
    pub scale: f64,
    /// The blocks it's compressed in and its resolution.
    pub quality: Quality,
}

/// A screen's picture, made.
pub struct Made {
    pub picture: Astc,
    /// How long it took to decode the wallpaper, to lay it out, and to compress it.
    pub took: [Duration; 3],
}

/// Makes the picture `recipe` says.
pub fn make(recipe: &Recipe) -> Result<Made, String> {
    let started = Instant::now();
    let laid = lay_out(recipe)?;
    let laid_out = Instant::now();
    let picture = astc::encode(&laid.image, recipe.quality.blocks())?;
    let took = [
        laid.decoded,
        laid_out
            .saturating_duration_since(started)
            .saturating_sub(laid.decoded),
        laid_out.elapsed(),
    ];
    Ok(Made { picture, took })
}

/// A screen's picture before it's compressed.
pub(super) struct Laid {
    pub(super) image: CFRetained<CGImage>,
    /// How long decoding the wallpaper took.
    decoded: Duration,
}

/// The wallpaper `recipe` names, laid out on its screen, in a bitmap the screen's size at the
/// recipe's resolution.
pub(super) fn lay_out(recipe: &Recipe) -> Result<Laid, String> {
    let started = Instant::now();
    let path = &recipe.wallpaper.path;
    let probe = picture::probe(path)?;
    let density = f64::from(probe.density);
    let points = (
        f64::from(probe.pixels.0) / density,
        f64::from(probe.pixels.1) / density,
    );
    let placed = layout::place(points, recipe.screen, recipe.wallpaper.scaling);
    let scale = recipe.scale * recipe.quality.resolution();
    let longest = layout::decode_longest(probe.pixels, placed, scale);
    let image = picture::decoded(path, recipe.wallpaper.image.unwrap_or(probe.index), longest)?;
    let decoded = started.elapsed();

    let pixels = |points: f64| (points * scale).round().max(1.) as u32;
    let canvas = Canvas::new(pixels(recipe.screen.0), pixels(recipe.screen.1))
        .ok_or("there's no memory to lay it out")?;
    let context = canvas.context();
    upside_down(context, canvas.height);
    if probe.see_through || !placed.covers(recipe.screen) {
        canvas.fill(canvas.bounds(), recipe.wallpaper.fill.map(CGFloat::from));
    }
    // Quartz counts from the bottom left, the layout from the top left.
    let height = CGFloat::from(canvas.height);
    let rect = CGRect::new(
        CGPoint::new(
            placed.x * scale,
            height - (placed.y + placed.height) * scale,
        ),
        CGSize::new(placed.width * scale, placed.height * scale),
    );
    CGContext::draw_image(Some(context), rect, Some(&image));
    let image = CGBitmapContextCreateImage(Some(context)).ok_or("its pixels can't be read")?;
    Ok(Laid { image, decoded })
}

/// Has what's drawn with `context`, `height` pixels tall, land upside down in its bitmap.
/// `ImageIO`'s ASTC encoder writes a picture's bottom row first, as OpenGL counts rows, and the
/// GPU reads the first row of blocks as the top one: drawn upside down, the picture reaches it
/// upright.
pub(super) fn upside_down(context: &CGContext, height: u32) {
    CGContext::translate_ctm(Some(context), 0., CGFloat::from(height));
    CGContext::scale_ctm(Some(context), 1., -1.);
}
