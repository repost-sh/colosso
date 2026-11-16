//! The backdrop's tests: a picture made from a file, compressed, read back in the order the GPU
//! reads it, and drawn by the renderer as a chat draws it; and how long making one takes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use objc2_core_foundation::{
    CFData, CFRetained, CFString, CFURL, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGContext, CGImage};
use objc2_image_io::{CGImageDestination, CGImageSource};

use super::make::{Recipe, fill, lay_out, make, upside_down};
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
        upside_down(canvas.context(), height);
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
    let reference = lay_out(&at(Blocks::Eight, 1.)).unwrap();
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

/// What the renderer draws of `placeholder` painted over all of a frame `width` by `height`,
/// shown only where `shown` is, as red, green, blue and alpha a pixel.
fn drawn(
    placeholder: &CFRetained<objc2_core_video::CVPixelBuffer>,
    (width, height): (u32, u32),
    shown: gpui_kit::Bounds<gpui_kit::ScaledPixels>,
) -> image::RgbaImage {
    use gpui_apple::metal_renderer::{Context, MetalRenderer};
    use gpui_kit::{
        Bounds, ContentMask, DevicePixels, PaintSurface, Scene, ScaledPixels, point, size,
    };
    let mut scene = Scene::default();
    let frame = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
    );
    scene.insert_primitive(PaintSurface {
        order: 0,
        bounds: frame,
        content_mask: ContentMask { bounds: shown },
        image_buffer: placeholder.clone(),
    });
    scene.finish();
    let mut renderer = MetalRenderer::new_headless(Context::default());
    let size = size(DevicePixels(width as i32), DevicePixels(height as i32));
    renderer.render_scene_to_image(&scene, size).unwrap()
}

#[test]
fn the_gpu_draws_a_screens_picture_as_it_was_laid_out_and_only_where_its_pane_is() {
    use gpui_kit::{Bounds, ScaledPixels, point, size};

    use super::texture::Texture;

    let folder = scratch("chat-backdrop-gpu");
    let path = folder.join("wallpaper.png");
    picture_file(&path, 300, 200, "public.png");
    let recipe = recipe(path, (96., 64.), Blocks::Eight, 1.);
    let made = make(&recipe).unwrap();
    let texture = Texture::new(&made.picture).unwrap();
    assert_eq!(texture.bytes, 24 * 16 * 16);
    let placeholder = texture.placeholder().unwrap();
    let whole = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(192.), ScaledPixels(128.)),
    );
    let image = drawn(&placeholder, (192, 128), whole);
    let pixel = |x: u32, y: u32| {
        let [r, g, b, _] = image.get_pixel(x, y).0;
        [r, g, b]
    };
    // Dimmed half way to black and read as it's held, as GPUI's own images are: 128 stays 128.
    // Taken out of sRGB as it's sampled, it would be 55.
    let near = |got: [u8; 3], expected: [u8; 3]| {
        got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 12)
    };
    assert!(near(pixel(4, 4), [128, 0, 0]), "{:?}, red at the top", pixel(4, 4));
    assert!(near(pixel(4, 123), [0, 0, 128]), "{:?}, blue below", pixel(4, 123));
    assert!(near(pixel(96, 64), [128, 128, 128]), "{:?}, the bar", pixel(96, 64));

    // Clipped to the left half, as to a pane: nothing right of it, where the headless frame
    // stays the black it's cleared to.
    let left = Bounds::new(whole.origin, size(ScaledPixels(96.), ScaledPixels(128.)));
    let image = drawn(&placeholder, (192, 128), left);
    let pixel = |x: u32, y: u32| {
        let [r, g, b, _] = image.get_pixel(x, y).0;
        [r, g, b]
    };
    assert_eq!(pixel(150, 4), [0, 0, 0], "past the pane");
    assert!(near(pixel(40, 4), [128, 0, 0]), "in the pane");

    // Let go, it draws nothing.
    drop(texture);
    let image = drawn(&placeholder, (192, 128), whole);
    assert_eq!(image.get_pixel(4, 4).0[..3], [0, 0, 0]);
}
