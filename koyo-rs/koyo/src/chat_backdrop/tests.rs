//! The backdrop's tests: a picture made from a file, compressed, read back in the order the GPU
//! reads it, and drawn by the renderer as a chat draws it, dithered and faded; how long making
//! one takes; and when the watch looks at the wallpapers and lets a picture go.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{AppContext as _, TestAppContext, px, size};
use objc2_core_foundation::{
    CFData, CFRetained, CFString, CFURL, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{CGBitmapContextCreateImage, CGContext, CGImage};
use objc2_image_io::{CGImageDestination, CGImageSource};

use super::astc::{self, Astc};
use super::make::{Recipe, lay_out, make, upside_down};
use super::screens;
use super::texture::Texture;
use crate::budget::wakes_a_minute;
use crate::chat_backdrop::layout::Scaling;
use crate::chat_backdrop::wallpaper::Wallpaper;
use crate::settings::{ChatBackdrop, Quality};
use crate::test_support::{Blank, in_window, on_screen, scratch};
use crate::viewer::bitmap::Canvas;

/// Red over the top half of `canvas`, blue under, a white bar down its middle third.
fn striped(canvas: &Canvas) {
    let (w, h) = (CGFloat::from(canvas.width), CGFloat::from(canvas.height));
    let rect = |x: f64, y: f64, w: f64, h: f64| CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
    canvas.fill(rect(0., h / 2., w, h / 2.), [1., 0., 0.]);
    canvas.fill(rect(0., 0., w, h / 2.), [0., 0., 1.]);
    canvas.fill(rect(w / 3., 0., w / 3., h), [1., 1., 1.]);
}

/// Grey half way to white, all over `canvas`.
fn grey(canvas: &Canvas) {
    canvas.fill(canvas.bounds(), [0.5, 0.5, 0.5]);
}

/// A picture `width` by `height` saved at `path` as `kind`, as `paint` paints it.
fn picture_file(path: &Path, width: u32, height: u32, kind: &str, paint: fn(&Canvas)) {
    let canvas = Canvas::new(width, height).unwrap();
    paint(&canvas);
    let image = CGBitmapContextCreateImage(Some(canvas.context())).unwrap();
    let url = CFURL::from_file_path(path).unwrap();
    let kind = CFString::from_str(kind);
    // SAFETY: a type ImageIO names, for one image, with no options.
    unsafe {
        let destination = CGImageDestination::with_url(&url, &kind, 1, None).unwrap();
        destination.add_image(&image, None);
        assert!(destination.finalize());
    }
}

fn recipe(path: PathBuf, screen: (f64, f64), quality: Quality) -> Recipe {
    Recipe {
        wallpaper: Wallpaper {
            path,
            modified: None,
            scaling: Scaling::Fill,
            image: None,
            fill: [0., 0., 0.],
        },
        screen,
        scale: 2.,
        quality,
    }
}

/// `astc` decoded back by `ImageIO`, its rows in the order the file holds them, which is the
/// order the GPU reads them in: `ImageIO` turns them over as it writes a file, not as it reads
/// one.
fn decoded_image(astc: &Astc) -> CFRetained<CGImage> {
    let mut file = astc::header(astc.blocks.side() as u8, astc.width, astc.height);
    file.extend(&astc.data);
    let data = CFData::from_bytes(&file);
    // SAFETY: a source over the bytes, asked for its first image.
    unsafe {
        CGImageSource::with_data(&data, None)
            .unwrap()
            .image_at_index(0, None)
    }
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
    canvas
        .into_frame(false, None)
        .unwrap()
        .into_buffer()
        .into_raw()
}

/// `astc` decoded, as blue, green, red and alpha a pixel, and how wide it is.
fn decoded(astc: &Astc) -> (u32, Vec<u8>) {
    let image = decoded_image(astc);
    (
        astc.width,
        pixels_of(&image, astc.width, astc.height, false),
    )
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
fn a_screens_picture_is_laid_out_upright_and_compressed() {
    let folder = scratch("chat-backdrop-make");
    let path = folder.join("wallpaper.png");
    picture_file(&path, 300, 200, "public.png", striped);
    for quality in Quality::ALL {
        let made = make(&recipe(path.clone(), (96., 64.), quality)).unwrap();
        let resolution = quality.resolution();
        let picture = &made.picture;
        let side = (192. * resolution) as u32;
        assert_eq!((picture.blocks, picture.width), (quality.blocks(), side));
        assert_eq!(picture.height, (128. * resolution) as u32);
        let (width, pixels) = decoded(picture);
        let pixel = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            [pixels[at + 2], pixels[at + 1], pixels[at]]
        };
        let near = |got: [u8; 3], expected: [u8; 3]| {
            got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 24)
        };
        let (w, h) = (picture.width, picture.height);
        assert!(
            near(pixel(4, 4), [255, 0, 0]),
            "{:?} at the top",
            pixel(4, 4)
        );
        assert!(
            near(pixel(4, h - 5), [0, 0, 255]),
            "{:?} below",
            pixel(4, h - 5)
        );
        assert!(
            near(pixel(w / 2, h / 2), [255, 255, 255]),
            "{:?}",
            pixel(w / 2, h / 2)
        );
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
            picture_file(&path, 6016, 6016, "public.heic", striped);
            path
        },
        PathBuf::from,
    );
    let ms = |took: Duration| took.as_secs_f64() * 1000.;
    let at = |quality| recipe(path.clone(), (2560., 1440.), quality);
    let (width, height) = (5120, 2880);
    let reference = lay_out(&at(Quality::Astc8Full)).unwrap();
    let reference = pixels_of(&reference.image, width, height, true);
    for quality in Quality::ALL {
        let name = format!("{quality:?}");
        let made = make(&at(quality)).unwrap();
        let shown = pixels_of(&decoded_image(&made.picture), width, height, false);
        println!(
            "{name}: {:.1} dB against the picture uncompressed",
            psnr(&shown, &reference)
        );
        for _ in 0..3 {
            let made = make(&at(quality)).unwrap();
            let [decoded, laid_out, compressed] = made.took;
            println!(
                "{name}: decoded {:.0} ms, laid out {:.0} ms, compressed {:.0} ms, \
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
        Bounds, ContentMask, DevicePixels, PaintSurface, ScaledPixels, Scene, point, size,
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
    let mut renderer = MetalRenderer::new_headless(Context::default()).unwrap();
    let size = size(DevicePixels(width as i32), DevicePixels(height as i32));
    renderer.render_scene_to_image(&scene, size).unwrap()
}

/// The frame `drawn` draws, all of it shown.
fn whole() -> gpui_kit::Bounds<gpui_kit::ScaledPixels> {
    use gpui_kit::{Bounds, ScaledPixels, point, size};
    Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(192.), ScaledPixels(128.)),
    )
}

/// The picture of a 96 × 64 pt screen at 2× made from a 300 × 200 file `paint` paints, put on
/// the GPU, and a pixel standing for it.
fn on_the_gpu(
    name: &str,
    paint: fn(&Canvas),
) -> (Texture, CFRetained<objc2_core_video::CVPixelBuffer>) {
    let path = scratch(name).join("wallpaper.png");
    picture_file(&path, 300, 200, "public.png", paint);
    let made = make(&recipe(path, (96., 64.), Quality::Astc8Full)).unwrap();
    let texture = Texture::new(&made.picture).unwrap();
    assert_eq!(texture.bytes, 24 * 16 * 16);
    (texture, Texture::placeholder().unwrap())
}

/// A look with neither a fade nor any see-through, so what's drawn is the dither alone.
fn solid() -> ChatBackdrop {
    ChatBackdrop {
        opacity: 1.,
        fade: false,
        ..ChatBackdrop::default()
    }
}

/// Red, green and blue at `x`, `y` of `image`.
fn rgb(image: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let [red, green, blue, _] = image.get_pixel(x, y).0;
    [red, green, blue]
}

fn near(got: [u8; 3], expected: [u8; 3]) -> bool {
    got.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 12)
}

#[test]
fn the_gpu_draws_a_screens_picture_where_it_was_laid_out_and_only_where_its_pane_is() {
    use gpui_kit::{Bounds, ScaledPixels, size};

    let (texture, placeholder) = on_the_gpu("chat-backdrop-gpu", striped);
    texture.mark(&placeholder, &solid());
    let image = drawn(&placeholder, (192, 128), whole());
    assert!(
        near(rgb(&image, 4, 4), [255, 0, 0]),
        "{:?}, red at the top",
        rgb(&image, 4, 4)
    );
    assert!(
        near(rgb(&image, 4, 123), [0, 0, 255]),
        "{:?}, blue below",
        rgb(&image, 4, 123)
    );
    assert!(
        near(rgb(&image, 96, 64), [255, 255, 255]),
        "{:?}, the bar",
        rgb(&image, 96, 64)
    );

    let left = Bounds::new(whole().origin, size(ScaledPixels(96.), ScaledPixels(128.)));
    let image = drawn(&placeholder, (192, 128), left);
    assert_eq!(rgb(&image, 150, 4), [0, 0, 0], "past the pane");
    assert!(near(rgb(&image, 40, 4), [255, 0, 0]), "in the pane");

    drop(texture);
    let image = drawn(&placeholder, (192, 128), whole());
    assert_eq!(rgb(&image, 4, 4), [0, 0, 0]);
}

#[test]
fn a_mid_grey_lights_half_its_cells_at_full_brightness_and_the_rest_near_black() {
    let (texture, placeholder) = on_the_gpu("chat-backdrop-dither", grey);
    let lit = |density: f32| {
        let look = ChatBackdrop { density, ..solid() };
        texture.mark(&placeholder, &look);
        let image = drawn(&placeholder, (192, 128), whole());
        for (x, y) in [(0, 0), (2, 0), (4, 6), (10, 12)] {
            let cell =
                [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)].map(|(x, y)| rgb(&image, x, y));
            assert!(cell.iter().all(|pixel| *pixel == cell[0]), "{cell:?}");
        }
        let pixels: Vec<[u8; 3]> = image
            .pixels()
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        let bright = pixels
            .iter()
            .filter(|pixel| near(**pixel, [255, 255, 255]))
            .count();
        let dark = pixels
            .iter()
            .filter(|pixel| near(**pixel, [10, 10, 10]))
            .count();
        assert_eq!(
            bright + dark,
            pixels.len(),
            "lit at full brightness or at 8 %, nothing else"
        );
        bright as f32 / pixels.len() as f32
    };
    let half = lit(1.);
    assert!((half - 0.5).abs() < 0.02, "{half} of the cells lit");
    let more = lit(1.5);
    assert!((more - 0.75).abs() < 0.02, "{more} lit, denser");
}

#[test]
fn it_shows_at_its_opacity_and_fades_out_down_its_pane() {
    let (texture, placeholder) = on_the_gpu("chat-backdrop-fade", striped);
    let faint = ChatBackdrop {
        opacity: 0.5,
        ..solid()
    };
    texture.mark(&placeholder, &faint);
    let image = drawn(&placeholder, (192, 128), whole());
    assert!(
        near(rgb(&image, 4, 4), [128, 0, 0]),
        "{:?}, half of red",
        rgb(&image, 4, 4)
    );

    let fading = ChatBackdrop {
        fade: true,
        fade_start: 0.,
        fade_end: 1.,
        ..solid()
    };
    texture.mark(&placeholder, &fading);
    let image = drawn(&placeholder, (192, 128), whole());
    let red = |y: u32| rgb(&image, 4, y)[0];
    assert!(red(0) > 240, "whole where it starts: {}", red(0));
    assert!(red(32).abs_diff(158) <= 20, "a quarter down: {}", red(32));
    assert!(red(127) < 8, "gone at the bottom: {}", red(127));
}

/// The watch looks at the wallpapers once a second only while a chat shows the backdrop in a
/// window somebody can see, and otherwise sleeps until told: by a chat showing it, or by a window
/// seen again after nobody could see one, as the shell tells it.
#[gpui_kit::test]
fn the_wallpapers_are_looked_at_only_while_a_backdrop_is_seen(cx: &mut TestAppContext) {
    cx.update(super::init);
    let (window, _) = in_window(size(px(400.), px(300.)), cx, |_, cx| cx.new(|_| Blank));
    assert_eq!(wakes_a_minute(cx), 0, "no chat shows the backdrop");
    let hold = cx.update(|cx| screens::hold(1, cx));
    assert_eq!(wakes_a_minute(cx), 60, "a chat shows it in a window seen");
    on_screen(window, false, cx);
    assert_eq!(wakes_a_minute(cx), 0, "nobody can see the window");
    on_screen(window, true, cx);
    cx.update(|cx| super::look_again(cx));
    assert_eq!(wakes_a_minute(cx), 60, "the window seen again");
    drop(hold);
    assert_eq!(wakes_a_minute(cx), 0, "no chat shows it any more");
}

/// A screen's picture no chat shows any more is let go 30 s on, with no window to look at the
/// wallpapers for meanwhile: the watch wakes for it then.
#[gpui_kit::test]
fn a_picture_no_chat_shows_is_let_go_a_while_on(cx: &mut TestAppContext) {
    cx.update(super::init);
    let path = scratch("chat-backdrop-let-go").join("wallpaper.png");
    picture_file(&path, 64, 64, "public.png", grey);
    let recipe = recipe(path, (64., 64.), Quality::Astc8Full);
    let shown = |cx: &mut TestAppContext| {
        cx.update(|cx| screens::picture(1, Some(recipe.clone()), solid(), cx)).is_some()
    };
    let hold = cx.update(|cx| screens::hold(1, cx));
    assert!(!shown(cx), "being made");
    cx.run_until_parked();
    assert!(shown(cx), "made");
    drop(hold);
    cx.executor().advance_clock(Duration::from_secs(29));
    cx.run_until_parked();
    assert!(shown(cx), "kept a while once no chat shows it");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(!shown(cx), "let go");
}
