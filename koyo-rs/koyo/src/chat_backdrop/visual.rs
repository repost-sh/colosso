//! `--visual-check-chat-backdrop` (`docs/chat-backdrop.md` 10, step 9): a chat over a wallpaper
//! kept in the checkout, not the person's, drawn by the real shell in a real window off screen and
//! saved as PNGs under `.artifacts/qa/chat-backdrop/`: the backdrop on and off, in the light
//! appearance and the dark, at each Quality stop (with a crop of each at a pixel a pixel, where
//! the picture is sharp), the Orca layout's sidebar, tabs and Files panel over it, its compact
//! mode, the tabs still in its top rail, and Appearance › Chat background. A window off screen lies on no screen,
//! so the check's window stands on one of its own, 2560 × 1440 points, at (480, 220).
//!
//! ```sh
//! Q=$PWD/.artifacts/qa/chat-backdrop && mkdir -p $Q/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp KOYO_CONFIG_DIR=$Q/config \
//!   KOYO_LOG_DIR=$Q/logs script/cargo run -p koyo --features visual-check -- \
//!   --visual-check-chat-backdrop
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui_kit::{Bounds, Pixels, point, px, size};
use image::{Rgb, RgbImage};

use super::layout::Scaling;
use super::screens;
use super::wallpaper::Named;
use crate::navigation::Route;
use crate::right_panel::View;
use crate::settings::{self, Quality};
use crate::settings_view::SettingsPage;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout, out_dir};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".artifacts/qa/chat-backdrop";
/// The display ID of the check's own screen, which no real one has.
const DISPLAY: u64 = 0xc4ec;
/// The check's screen, in points, and where its window lies on it.
const SCREEN: (f32, f32) = (2560., 1440.);
const WINDOW_AT: (f32, f32) = (480., 220.);
/// How long a picture gets to be made, with the app slow to draw.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the chat gets to take in what the fake provider said back.
const ANSWERED_IN: Duration = Duration::from_secs(3);
/// What's told to the chat, and so said back: enough lines to reach where it fades.
const SAID: &str = "\
The backdrop is the wallpaper behind the window, dithered in its own colours.

- Each cell is two screen pixels, lit at full brightness or at 8 percent.
- It shows faintly over the card, and fades out towards the bottom.
- The composer floats over where it has faded.

Each screen's picture is made once, compressed to ASTC, and only moved as the window moves.";

/// The wallpaper the check stands its window's screen on, while it runs.
static STAND_IN: Mutex<Option<Named>> = Mutex::new(None);

/// The check's own screen and where it lies relative to the window, while the check runs.
pub fn screen() -> Option<(u64, Bounds<Pixels>)> {
    STAND_IN.lock().ok()?.as_ref()?;
    let origin = point(px(-WINDOW_AT.0), px(-WINDOW_AT.1));
    Some((
        DISPLAY,
        Bounds::new(origin, size(px(SCREEN.0), px(SCREEN.1))),
    ))
}

/// The wallpaper of the check's own screen, while the check runs.
pub fn wallpaper() -> Option<Named> {
    STAND_IN.lock().ok()?.clone()
}

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let out = out_dir(OUT)?;
    let path = out.join("wallpaper.png");
    paint_wallpaper(&path)?;
    *STAND_IN.lock().map_err(|error| error.to_string())? = Some(Named {
        path,
        scaling: Scaling::Fill,
        fill: [0., 0., 0.],
    });
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    drawn.chat_saying(&checkout()?, SAID)?;
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| {
        Ok(start.elapsed() >= ANSWERED_IN)
    })?;

    let mut saved = vec![settled(&mut drawn, "light.png")?];
    drawn.cx.update(|cx| cx.dispatch_action(&UseDarkTheme));
    saved.push(settled(&mut drawn, "dark.png")?);
    drawn.cx.update(|cx| cx.dispatch_action(&UseLightTheme));
    for quality in Quality::ALL {
        set(&mut drawn, |backdrop| backdrop.quality = quality);
        let name = format!("{quality:?}").to_lowercase();
        let shown = settled(&mut drawn, &format!("quality-{name}.png"))?;
        saved.push(crop(&drawn, &shown, &format!("quality-{name}-crop.png"))?);
        saved.push(shown);
    }
    set(&mut drawn, |backdrop| backdrop.quality = Quality::default());
    for (dark, name) in [(false, "off-light.png"), (true, "off-dark.png")] {
        drawn.cx.update(|cx| {
            settings::update(cx, |settings| settings.chat_backdrop.on = false);
            if dark {
                cx.dispatch_action(&UseDarkTheme);
            }
        });
        saved.push(drawn.save(name)?);
    }
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.chat_backdrop.on = true);
        let shell = drawn.shell.clone();
        shell.update(cx, |shell, cx| shell.show(View::Files, cx));
    });
    saved.push(settled(&mut drawn, "orca-files-dark.png")?);
    let shell = drawn.shell.clone();
    for open in [false, true] {
        drawn.cx.update(|cx| {
            cx.set_reduce_motion(!open);
            shell.update(cx, |shell, cx| shell.set_sidebar_open(open, cx));
        });
        if !open {
            saved.push(settled(&mut drawn, "orca-compact-dark.png")?);
        }
    }
    drawn.cx.update(|cx| {
        cx.dispatch_action(&UseLightTheme);
        let shell = drawn.shell.clone();
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::Appearance), cx);
        });
    });
    saved.push(drawn.save("settings-appearance.png")?);
    Ok((saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect())
}

/// Changes the backdrop's settings as `change` does.
fn set(drawn: &mut Drawn, change: impl FnOnce(&mut settings::ChatBackdrop)) {
    drawn
        .cx
        .update(|cx| settings::update(cx, |settings| change(&mut settings.chat_backdrop)));
}

/// Saves the window as `name` once the screen's picture is the one asked for.
fn settled(drawn: &mut Drawn, name: &str) -> Result<PathBuf> {
    drawn.until(PATIENCE, |drawn| {
        Ok(drawn.cx.read(|cx| screens::settled(DISPLAY, cx)))
    })?;
    drawn.save(name)
}

/// A part of the picture saved at `shown`, where the backdrop is sharp, as `name`, a pixel a
/// pixel.
fn crop(drawn: &Drawn, shown: &Path, name: &str) -> Result<PathBuf> {
    let image = image::open(shown)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let (width, height) = (image.width(), image.height());
    let part = image::imageops::crop_imm(&image, width / 4, height / 10, width / 3, height / 4);
    drawn.save_image(&part.to_image(), name)
}

/// A wallpaper of the check's own at `path`: a diagonal from amber to blue, soft discs of colour,
/// a grid of fine lines and a band of finer checks, so the dither, the fade and the blocks each
/// have something to show on.
fn paint_wallpaper(path: &Path) -> Result<()> {
    let (width, height) = (5120u32, 2880u32);
    let discs = [
        (0.2, 0.3, 0.18, [255., 70., 90.]),
        (0.7, 0.25, 0.22, [60., 220., 160.]),
        (0.55, 0.75, 0.25, [250., 230., 60.]),
    ];
    let image = RgbImage::from_fn(width, height, |x, y| {
        let (u, v) = (x as f32 / width as f32, y as f32 / height as f32);
        let t = f32::midpoint(u, v);
        let mut colour = [
            255. * (1. - t) + 30. * t,
            160. * (1. - t) + 80. * t,
            40. * (1. - t) + 220. * t,
        ];
        for (cx, cy, radius, tint) in discs {
            let distance = ((u - cx) * 16. / 9.).hypot(v - cy) / radius;
            let weight = (1. - distance).clamp(0., 1.).powi(2);
            for (channel, tint) in colour.iter_mut().zip(tint) {
                *channel = *channel * (1. - weight) + tint * weight;
            }
        }
        if x % 160 < 2 || y % 160 < 2 {
            colour = [255.; 3];
        }
        if (0.4..0.46).contains(&v) && ((x / 2) + (y / 2)) % 2 == 0 {
            colour = colour.map(|channel| channel * 0.4);
        }
        Rgb(colour.map(|channel| channel.clamp(0., 255.) as u8))
    });
    image.save(path).map_err(|error| error.to_string())
}
