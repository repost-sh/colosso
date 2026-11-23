//! The downloads button in the top bar and the list it opens, as the app draws them, in the Arc
//! and the Orca layout, light and dark: a page shown, its address field with no downloads
//! button; the button with a download coming in, its ring part full; and the list open under
//! it, with a download of each state. Saved as PNGs in `.artifacts/qa/downloads/`: the top
//! of the window, then the same with the list laid over it where it opens. The list's glass is
//! its fill alone, as an off-screen window has no blur.
//!
//! ```sh
//! Q=$PWD/.artifacts/qa/downloads && mkdir -p $Q/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp \
//!   KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-downloads
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::Action;
use image::{RgbaImage, imageops};

use super::{Downloads, Entry, Key, Stage};
use crate::browser::profiles::By;
use crate::model::{Browser, Content, Pane, Tab, Unloaded};
use crate::popup::{self, Kind};
use crate::settings::{self, Layout};
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".artifacts/qa/downloads";
/// How much of the window's top a picture keeps, in points: the top bar, the address field
/// under it, and the list.
const ACROSS: u32 = 760;
const DOWN: u32 = 560;
/// The room a callout keeps from the window's edge (`menu::GAP`): the list is laid where it
/// opens over a window on screen; an off-screen window's floats are kept on the screen.
const GAP: f32 = 4.;
/// A frame of the display's, and enough of them for the button's arrival to have played.
const FRAME: Duration = Duration::from_millis(100);
const ARRIVAL_FRAMES: usize = 8;
/// How much the system's glass blurs what's behind it, as a Gaussian's sigma in pixels.
const BLUR: f32 = 14.;

pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

/// A download in the list, `received` of `total` bytes, as `stage` stands.
fn entry(id: u32, name: &str, (received, total): (u64, Option<u64>), stage: Stage, by: Option<&str>) -> Entry {
    Entry {
        key: Key::Engine { pane: 1, id },
        name: name.to_owned(),
        path: Some(PathBuf::from("/Users/me/Downloads").join(name)),
        received,
        total,
        stage,
        by: by.map(str::to_owned),
    }
}

/// A download of each state, the latest coming in.
fn listed() -> Vec<Entry> {
    vec![
        entry(1, "notes.txt", (1_200, Some(48_000)), Stage::Canceled, None),
        entry(2, "dataset.zip", (3_400_000, Some(9_000_000)), Stage::Failed("The network went away.".into()), None),
        entry(3, "report.csv", (18_400, Some(18_400)), Stage::Done, Some("claude-code:7f3a")),
        entry(4, "screenshot.png", (1_240_000, Some(1_240_000)), Stage::Done, None),
        entry(5, "file-sample_150kB.pdf", (61_000, Some(142_786)), Stage::Going, None),
    ]
}

fn web_page(address: &str, title: &str) -> Tab {
    let unloaded = Unloaded {
        address: address.into(),
        favicon: None,
        restored: true,
        session: koyo_browser_ipc::Session::default(),
    };
    Tab::Single(Pane {
        title: title.to_owned().into(),
        ..Pane::of(Content::Browser(Browser::waiting(unloaded, By::Person)))
    })
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.workspace.push_tab(web_page("https://file-examples.com/sample-pdf", "Sample PDF"));
            if let Some(unit) = shell.workspace.branch_mut() {
                unit.active_tab = unit.tabs.len() - 1;
            }
            cx.notify();
        });
        let downloads = Downloads::global(cx);
        downloads.update(cx, |downloads, cx| {
            downloads.list = listed();
            downloads.arrivals = 5;
            cx.notify();
        });
    });
    let mut said = Vec::new();
    for layout in [Layout::Arc, Layout::Orca] {
        drawn.cx.update(|cx| settings::update(cx, |settings| settings.layout = layout));
        let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
        for (theme, shade) in themes {
            drawn.cx.update(|cx| cx.dispatch_action(theme));
            drawn.draw()?;
            let name = format!("{}-{shade}", layout.label().to_lowercase());
            said.push(open_and_save(&mut drawn, &name)?);
        }
    }
    said.extend(at_rest(&mut drawn)?);
    Ok(said)
}

/// The button once every download has ended, in the Arc layout, light and dark.
fn at_rest(drawn: &mut Drawn) -> Result<Vec<String>> {
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.layout = Layout::Arc);
        Downloads::global(cx).update(cx, |downloads, cx| {
            for entry in &mut downloads.list {
                if entry.stage == Stage::Going {
                    entry.received = entry.total.unwrap_or(entry.received);
                    entry.stage = Stage::Done;
                }
            }
            cx.notify();
        });
    });
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    let mut said = Vec::new();
    for (theme, shade) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        frames(drawn, 2)?;
        let window = picture(drawn)?;
        let scale = window.width() / 1600;
        let bar = imageops::crop_imm(&window, 0, 0, ACROSS * scale, 160 * scale).to_image();
        let path = drawn.save_image(&bar, &format!("arc-{shade}-done-bar.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    Ok(said)
}

/// Draws `count` of the display's frames, a tenth of a second apart, so what animates plays
/// through: a view that didn't change is drawn again only for a frame it asked for.
fn frames(drawn: &mut Drawn, count: usize) -> Result<()> {
    for _ in 0..count {
        drawn.cx.advance_clock(FRAME);
        (drawn.cx)
            .update_window(drawn.window, |_, window, cx| window.simulate_next_frame(cx))
            .map_err(|error| error.to_string())?;
        drawn.draw()?;
    }
    Ok(())
}

/// The window as drawn now.
fn picture(drawn: &mut Drawn) -> Result<RgbaImage> {
    drawn.draw()?;
    drawn.cx.capture_screenshot(drawn.window).map_err(|error| error.to_string())
}

/// `list` laid over `window` at `(x, y)` in pixels, on the blur the system puts behind it: what
/// lies under its shape blurred, then the list's own glass and content.
fn laid_over(window: &RgbaImage, list: &RgbaImage, (x, y): (u32, u32)) -> RgbaImage {
    let (wide, high) = (list.width().min(window.width() - x), list.height().min(window.height() - y));
    let under = imageops::crop_imm(window, x, y, wide, high).to_image();
    let blurred = imageops::blur(&under, BLUR);
    let mut laid = window.clone();
    for (dx, dy, pixel) in blurred.enumerate_pixels() {
        if list.get_pixel(dx, dy)[3] > 0 {
            laid.put_pixel(x + dx, y + dy, *pixel);
        }
    }
    imageops::overlay(&mut laid, list, i64::from(x), i64::from(y));
    laid
}

/// Saves the window's top as `name`-bar, then opens the list as ⌥⌘L does and saves it laid over
/// the window where it opens as `name`-list, and closes it.
fn open_and_save(drawn: &mut Drawn, name: &str) -> Result<String> {
    frames(drawn, ARRIVAL_FRAMES)?;
    let window = picture(drawn)?;
    let scale = window.width() / 1600;
    let top = |picture: &RgbaImage| {
        imageops::crop_imm(picture, 0, 0, ACROSS * scale, DOWN * scale).to_image()
    };
    let bar = drawn.save_image(&top(&window), &format!("{name}-bar.png"))?;
    let toggle = |drawn: &mut Drawn| {
        (drawn.cx)
            .update_window(drawn.window, |_, window, cx| {
                window.dispatch_action(Box::new(super::ToggleDownloads), cx);
            })
            .map_err(|error| error.to_string())
    };
    toggle(drawn)?;
    let window = picture(drawn)?;
    let menu = drawn.cx.read(|cx| popup::window(Kind::Menu, cx)).ok_or("⌥⌘L opened no list")?;
    (drawn.cx)
        .update_window(menu, |_, window, cx| window.draw(cx).clear(cx))
        .map_err(|error| error.to_string())?;
    let list = drawn.cx.capture_screenshot(menu).map_err(|error| error.to_string())?;
    let button = crate::visual::noted("downloads-button").ok_or("the button wasn't drawn")?;
    let wide = (list.width() / scale) as f32;
    let left = (button.center().x.as_f32() - wide / 2.).clamp(GAP, 1600. - wide - GAP);
    let below = button.bottom().as_f32() + 1.;
    let pixels = |at: f32| (at as u32) * scale;
    let laid = laid_over(&window, &list, (pixels(left), pixels(below)));
    let listed = drawn.save_image(&top(&laid), &format!("{name}-list.png"))?;
    toggle(drawn)?;
    drawn.draw()?;
    Ok(format!("saved {} and {}", bar.display(), listed.display()))
}
