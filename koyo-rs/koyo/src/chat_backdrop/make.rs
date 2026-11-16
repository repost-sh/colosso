//! Making a screen's picture, off the main thread: the wallpaper decoded by the system's
//! decoders at no more pixels than its layout needs, drawn into a bitmap the screen's size as
//! macOS lays it out, dimmed towards the card's colour, and compressed to ASTC, the only copy
//! kept. The bitmap is dropped once the blocks are out of it.

use std::time::{Duration, Instant};

use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGContext, CGImage};

use super::astc::{self, Astc, Blocks};
use super::layout;
use super::wallpaper::Wallpaper;
use crate::viewer::bitmap::Canvas;
use crate::viewer::picture;

/// What a screen's picture is made from: when any of it changes, the picture is made again.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub wallpaper: Wallpaper,
    /// The screen's size in points, and how many pixels make a point on it.
    pub screen: (f64, f64),
    pub scale: f64,
    /// The colour the picture is dimmed towards, the card's, in sRGB; and how far, from 0 for
    /// not at all to 1 for the card alone.
    pub card: [f32; 3],
    pub dim: f32,
    /// The blocks it's compressed in, and its pixels to the screen's: 1, or ½ for half.
    pub blocks: Blocks,
    pub resolution: f64,
}

/// A screen's picture, made.
pub struct Made {
    pub picture: Astc,
    /// How long it took to decode the wallpaper, to lay it out and dim it, and to compress it.
    pub took: [Duration; 3],
}

/// Makes the picture `recipe` says.
pub fn make(recipe: &Recipe) -> Result<Made, String> {
    let started = Instant::now();
    let laid = lay_out(recipe)?;
    let laid_out = Instant::now();
    let picture = astc::encode(&laid.image, recipe.blocks)?;
    let took = [
        laid.decoded,
        laid_out.saturating_duration_since(started).saturating_sub(laid.decoded),
        laid_out.elapsed(),
    ];
    Ok(Made { picture, took })
}

/// A screen's picture before it's compressed.
struct Laid {
    image: CFRetained<CGImage>,
    /// How long decoding the wallpaper took.
    decoded: Duration,
}

/// The wallpaper `recipe` names, laid out on its screen and dimmed, in a bitmap the screen's
/// size at the recipe's resolution.
fn lay_out(recipe: &Recipe) -> Result<Laid, String> {
    let started = Instant::now();
    let path = &recipe.wallpaper.path;
    let probe = picture::probe(path)?;
    let density = f64::from(probe.density);
    let points = (
        f64::from(probe.pixels.0) / density,
        f64::from(probe.pixels.1) / density,
    );
    let placed = layout::place(points, recipe.screen, recipe.wallpaper.scaling);
    let scale = recipe.scale * recipe.resolution;
    let longest = layout::decode_longest(probe.pixels, placed, scale);
    let image = picture::decoded(path, probe.index, longest)?;
    let decoded = started.elapsed();

    let pixels = |points: f64| (points * scale).round().max(1.) as u32;
    let canvas = Canvas::new(pixels(recipe.screen.0), pixels(recipe.screen.1))
        .ok_or("there's no memory to lay it out")?;
    let context = canvas.context();
    upside_down(context, canvas.height);
    if probe.see_through || !placed.covers(recipe.screen) {
        let [r, g, b] = recipe.wallpaper.fill;
        fill(context, canvas.bounds(), [r, g, b, 1.]);
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
    let [r, g, b] = recipe.card;
    fill(context, canvas.bounds(), [r, g, b, recipe.dim.clamp(0., 1.)]);
    let image = CGBitmapContextCreateImage(Some(context)).ok_or("its pixels can't be read")?;
    Ok(Laid { image, decoded })
}

/// Has what's drawn with `context`, `height` pixels tall, land upside down in its bitmap.
/// `ImageIO`'s ASTC encoder writes a picture's bottom row first, as OpenGL counts rows, and the
/// GPU reads the first row of blocks as the top one: drawn upside down, the picture reaches it
/// upright.
fn upside_down(context: &CGContext, height: u32) {
    CGContext::translate_ctm(Some(context), 0., CGFloat::from(height));
    CGContext::scale_ctm(Some(context), 1., -1.);
}

/// Paints `rect` of `context` in `colour`, red, green, blue and alpha, over what's there.
fn fill(context: &CGContext, rect: CGRect, colour: [f32; 4]) {
    let [r, g, b, a] = colour.map(CGFloat::from);
    CGContext::set_rgb_fill_color(Some(context), r, g, b, a);
    CGContext::fill_rect(Some(context), rect);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use objc2_core_foundation::{
        CFData, CFRetained, CFString, CFURL, CGFloat, CGPoint, CGRect, CGSize,
    };
    use objc2_core_graphics::{CGBitmapContextCreateImage, CGContext, CGImage};
    use objc2_image_io::{CGImageDestination, CGImageSource};

    use super::{Recipe, fill, make};
    use crate::chat_backdrop::astc::{Astc, Blocks};
    use crate::chat_backdrop::layout::Scaling;
    use crate::chat_backdrop::wallpaper::Wallpaper;
    use crate::test_support::scratch;
    use crate::viewer::bitmap::Canvas;

    /// A picture `width` by `height` saved at `path` as `kind`: red over its top half, blue
    /// under, with a white bar down its middle third.
    fn picture_file(path: &Path, width: u32, height: u32, kind: &str) {
        let canvas = Canvas::new(width, height).unwrap();
        let context = canvas.context();
        let (w, h) = (CGFloat::from(width), CGFloat::from(height));
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
        };
        // Quartz's y goes up: the top half is the upper rectangle.
        fill(context, rect(0., h / 2., w, h / 2.), [1., 0., 0., 1.]);
        fill(context, rect(0., 0., w, h / 2.), [0., 0., 1., 1.]);
        fill(context, rect(w / 3., 0., w / 3., h), [1., 1., 1., 1.]);
        let image = CGBitmapContextCreateImage(Some(context)).unwrap();
        let url = CFURL::from_file_path(path).unwrap();
        let kind = CFString::from_str(kind);
        // SAFETY: a type ImageIO names, for one image, with no options.
        unsafe {
            let destination = CGImageDestination::with_url(&url, &kind, 1, None).unwrap();
            destination.add_image(&image, None);
            assert!(destination.finalize());
        }
    }

    fn recipe(path: PathBuf, screen: (f64, f64), blocks: Blocks, resolution: f64) -> Recipe {
        Recipe {
            wallpaper: Wallpaper {
                path,
                modified: None,
                scaling: Scaling::Fill,
                fill: [0., 0., 0.],
            },
            screen,
            scale: 2.,
            card: [0., 0., 0.],
            dim: 0.5,
            blocks,
            resolution,
        }
    }

    /// `astc` decoded back by `ImageIO`, its rows in the order the file holds them, which is the
    /// order the GPU reads them in: `ImageIO` turns them over as it writes a file, not as it reads
    /// one.
    fn decoded_image(astc: &Astc) -> CFRetained<CGImage> {
        let side = astc.blocks.side() as u8;
        let mut file = vec![0x13, 0xab, 0xa1, 0x5c, side, side, 1];
        file.extend(&astc.width.to_le_bytes()[..3]);
        file.extend(&astc.height.to_le_bytes()[..3]);
        file.extend([1, 0, 0]);
        file.extend(&astc.data);
        let data = CFData::from_bytes(&file);
        // SAFETY: a source over the bytes, asked for its first image.
        unsafe { CGImageSource::with_data(&data, None).unwrap().image_at_index(0, None) }
            .expect("ImageIO reads ASTC back")
    }

    /// `image` drawn over a bitmap `width` by `height`, upside down if `flipped`, as blue,
    /// green, red and alpha a pixel.
    fn pixels_of(image: &CGImage, width: u32, height: u32, flipped: bool) -> Vec<u8> {
        let canvas = Canvas::new(width, height).unwrap();
        if flipped {
            super::upside_down(canvas.context(), height);
        }
        CGContext::draw_image(Some(canvas.context()), canvas.bounds(), Some(image));
        canvas.into_frame(false, None).unwrap().into_buffer().into_raw()
    }

    /// `astc` decoded, as blue, green, red and alpha a pixel, and how wide it is.
    fn decoded(astc: &Astc) -> (u32, Vec<u8>) {
        let image = decoded_image(astc);
        (astc.width, pixels_of(&image, astc.width, astc.height, false))
    }

    /// The peak signal to noise ratio of `got` against `expected`, over red, green and blue.
    fn psnr(got: &[u8], expected: &[u8]) -> f64 {
        let (mut sum, mut count) = (0f64, 0f64);
        for (got, expected) in got.chunks(4).zip(expected.chunks(4)) {
            for channel in 0..3 {
                let error = f64::from(got[channel]) - f64::from(expected[channel]);
                sum += error * error;
                count += 1.;
            }
        }
        10. * (255. * 255. / (sum / count)).log10()
    }

    #[test]
    fn a_screens_picture_is_laid_out_upright_dimmed_and_compressed() {
        let folder = scratch("chat-backdrop-make");
        let path = folder.join("wallpaper.png");
        picture_file(&path, 300, 200, "public.png");
        for (blocks, resolution) in [(Blocks::Eight, 1.), (Blocks::Four, 0.5)] {
            let made = make(&recipe(path.clone(), (96., 64.), blocks, resolution)).unwrap();
            let picture = &made.picture;
            let side = (192. * resolution) as u32;
            assert_eq!((picture.blocks, picture.width), (blocks, side));
            assert_eq!(picture.height, (128. * resolution) as u32);
            let (width, pixels) = decoded(picture);
            let pixel = |x: u32, y: u32| {
                let at = ((y * width + x) * 4) as usize;
                [pixels[at + 2], pixels[at + 1], pixels[at]]
            };
            // Dimmed half way to black: red at the top left, blue at the bottom left, the bar
            // grey down the middle.
            let near = |got: [u8; 3], expected: [u8; 3]| {
                got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 24)
            };
            let (w, h) = (picture.width, picture.height);
            assert!(near(pixel(4, 4), [128, 0, 0]), "{:?} at the top", pixel(4, 4));
            assert!(near(pixel(4, h - 5), [0, 0, 128]), "{:?} below", pixel(4, h - 5));
            assert!(near(pixel(w / 2, h / 2), [128, 128, 128]), "{:?}", pixel(w / 2, h / 2));
        }
    }

    /// How long making a 2560 × 1440 point screen's picture at 2× takes, at each Quality stop,
    /// from the wallpaper `KOYO_BACKDROP_WALLPAPER` names, or a 6016 px square HEIC like
    /// macOS's own without one (`docs/chat-backdrop.md` 9).
    #[test]
    #[ignore = "a measurement, run with --ignored --nocapture"]
    fn making_a_screens_picture_takes() {
        let path = std::env::var_os("KOYO_BACKDROP_WALLPAPER").map_or_else(
            || {
                let path = scratch("chat-backdrop-measure").join("wallpaper.heic");
                picture_file(&path, 6016, 6016, "public.heic");
                path
            },
            PathBuf::from,
        );
        let stops = [
            ("8 × 8, half", Blocks::Eight, 0.5),
            ("4 × 4, half", Blocks::Four, 0.5),
            ("8 × 8, full", Blocks::Eight, 1.),
            ("4 × 4, full", Blocks::Four, 1.),
        ];
        let ms = |took: Duration| took.as_secs_f64() * 1000.;
        let at = |blocks, resolution| Recipe {
            dim: 0.6,
            ..recipe(path.clone(), (2560., 1440.), blocks, resolution)
        };
        let (width, height) = (5120, 2880);
        let reference = super::lay_out(&at(Blocks::Eight, 1.)).unwrap();
        let reference = pixels_of(&reference.image, width, height, true);
        for (name, blocks, resolution) in stops {
            let made = make(&at(blocks, resolution)).unwrap();
            let shown = pixels_of(&decoded_image(&made.picture), width, height, false);
            println!("{name}: {:.1} dB against the picture uncompressed", psnr(&shown, &reference));
            for _ in 0..3 {
                let made = make(&at(blocks, resolution)).unwrap();
                let [decoded, laid_out, compressed] = made.took;
                println!(
                    "{name}: decoded {:.0} ms, laid out and dimmed {:.0} ms, compressed {:.0} ms, \
                     {:.1} MB",
                    ms(decoded),
                    ms(laid_out),
                    ms(compressed),
                    made.picture.data.len() as f64 / 1e6
                );
            }
        }
    }
}
