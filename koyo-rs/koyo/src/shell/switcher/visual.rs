//! The tab switcher in real windows (`docs/tab-switcher.md`): the real shell off screen on this
//! repository's own checkout with a new chat's tab and three files', each drawn so its picture
//! is taken, ⌃⇥ as typed, and what macOS says of the panel's window as ⌃ is held and let go:
//! unseen until ⌃ has been held, never with the keyboard, with no edge of its own, the glass
//! behind what GPUI draws. The panel's cards, each with its tab's picture, are saved in
//! `.superpowers/qa/switcher/`; the glass, which macOS draws, isn't in it. It says what a
//! picture adds to a frame.
//!
//! The chat is nearly all prompt, which is on glass: the window draws it in a layer over its
//! own frame, and presents the two apart. So each tab's frame is presented as the app presents
//! one, not only read back as a check's are, and the chat's picture is looked at for its
//! prompt: the pane that stands for the glass, and what's on it.
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/switcher && mkdir -p $Q/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp KOYO_CONFIG_DIR=$Q/config \
//!   KOYO_LOG_DIR=$Q/logs script/cargo run -p koyo --features visual-check -- \
//!   --visual-check-switcher
//! ```

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui_kit::{AnyWindowHandle, Modifiers, ModifiersChangedEvent, Rgba};
use image::RgbaImage;
use objc2::runtime::{AnyObject, NSObjectProtocol as _};
use objc2::{ClassType as _, msg_send};
use objc2_app_kit::{NSGlassEffectView, NSView, NSWindow, NSWindowStyleMask};

use super::HOLD;
use crate::files::Place;
use crate::model::PaneKind;
use crate::popup::{self, Kind};
use crate::shell::pictures;
use crate::theme;
use crate::visual::{self, Drawn, Result};

/// Where the picture goes, relative to the checkout.
const OUT: &str = ".superpowers/qa/switcher";
/// The files shown in turn, each in a tab of its own: the last is the tab shown.
const FILES: [&str; 3] = [
    "koyo-rs/koyo/src/shell.rs",
    "koyo-rs/koyo/src/popup.rs",
    "docs/tab-switcher.md",
];
/// Just short of how long ⌃ is held before the panel shows.
const NOT_YET: Duration = Duration::from_millis(10);
/// How many frames are timed with a picture taken, and how many without.
const FRAMES: usize = 40;
/// How long a picture is left to see whether the GPU is done with it, and the longest it's
/// waited for.
const SETTLES: Duration = Duration::from_millis(50);
const PATIENCE: Duration = Duration::from_secs(5);
/// How near a pixel of a picture is to a pane's fill to be of the pane, and how far from it to
/// be of what's on the pane, of 255 in any of its colours.
const OF_PANE: u8 = 2;
const ON_PANE: u8 = 40;
/// The fewest pixels of a new chat's picture that are of what's on its prompt: its button.
const MARKS: usize = 5;

/// Runs the check and says what it found, or why it failed.
pub fn run() -> ExitCode {
    visual::run(check)
}

fn check() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let mut lines = Vec::new();
    // A new chat, nearly all of it its prompt, which is on glass.
    let shell = drawn.shell.clone();
    (drawn.cx).update(|cx| shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx)));
    drawn.pause()?;
    lines.push(in_layers(&mut drawn)?);
    for file in FILES {
        let shell = drawn.shell.clone();
        (drawn.cx)
            .update_window(drawn.window, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.view_file(Place::file(file), window, cx);
                });
            })
            .map_err(|error| error.to_string())?;
        drawn.pause()?;
        // The frame that shows the tab, presented, takes its picture.
        taken(&mut drawn)?;
    }
    lines.push(cost(&mut drawn)?);

    // ⌃⇥: the switcher is up on the three tabs, the one before picked, its panel unseen.
    drawn.cx.simulate_keystrokes(drawn.window, "ctrl-tab");
    drawn.cx.run_until_parked();
    let panel =
        (drawn.cx.read(|cx| popup::window(Kind::Switcher, cx))).ok_or("⌃⇥ opened no switcher")?;
    // The check's app is never the one in front, and a window that hides while it isn't would
    // stay unseen whatever the switcher did.
    native(&mut drawn, panel, |window| {
        window.setHidesOnDeactivate(false);
    })?;
    expect(&state(&drawn)?, &(titles(), 1, false), "as ⌃⇥ is pressed")?;
    let made = made(&mut drawn, panel)?;
    expect(&made, &(true, true), "its own edge gone, the glass in")?;
    let unseen = (false, false);
    expect(&seen(&mut drawn, panel)?, &unseen, "as ⌃⇥ is pressed")?;
    drawn.cx.advance_clock(HOLD.saturating_sub(NOT_YET));
    drawn.cx.run_until_parked();
    expect(&seen(&mut drawn, panel)?, &unseen, "just before the hold")?;

    // ⌃ held: it shows, and still hasn't the keyboard.
    drawn.cx.advance_clock(NOT_YET);
    drawn.cx.run_until_parked();
    expect(&seen(&mut drawn, panel)?, &(true, false), "after the hold")?;
    expect(&state(&drawn)?, &(titles(), 1, true), "after the hold")?;
    let picture = drawn.save_window(panel, "switcher.png")?;
    lines.push(format!("saved {}", picture.display()));

    // ⌃⇥ again, heard in the window behind, moves the pick on; ⌃ let go there shows the tab.
    drawn.cx.simulate_keystrokes(drawn.window, "ctrl-tab");
    expect(&state(&drawn)?, &(titles(), 2, true), "after ⌃⇥ again")?;
    let released = ModifiersChangedEvent {
        modifiers: Modifiers::none(),
        capslock: gpui_kit::Capslock::default(),
    };
    drawn.cx.simulate_event(drawn.window, released);
    drawn.draw()?;
    let up = drawn.cx.read(|cx| popup::window(Kind::Switcher, cx));
    expect(&up, &None, "the switcher once ⌃ is let go")?;
    let shown = drawn.cx.read(|cx| {
        let tab = drawn.shell.read(cx).workspace.tab_shown()?;
        Some(tab.first().title.to_string())
    });
    let picked = Some("shell.rs");
    expect(&shown.as_deref(), &picked, "the tab shown once ⌃ is let go")?;
    lines.push(
        "each tab's picture was taken; the panel was unseen until ⌃ had been held, never had \
         the keyboard, had no edge of its own and glass behind it; ⌃ let go showed the tab picked"
            .into(),
    );
    Ok(lines)
}

/// The picture of the chat shown, as the app takes it: of a frame the window presents in two
/// layers, its prompt on glass in the upper. It has the pane that stands for the glass, a
/// hundredth of it at least, and what's on the pane over it; it's saved.
fn in_layers(drawn: &mut Drawn) -> Result<String> {
    let glass = native(drawn, drawn.window, has_glass)?;
    let layered = "glass in the shell's window, so drawn in layers";
    expect(&glass, &true, layered)?;
    let picture = taken(drawn)?;
    let saved = drawn.save_image(&picture, "chat-picture.png")?;
    let fill = (drawn.cx).read(|cx| Rgba::from(theme::palette(cx).floating()));
    let fill = [fill.r, fill.g, fill.b].map(|part| (part * 255.).round() as u8);
    let (pane, marks) = on_pane(&picture, fill);
    let all = picture.pixels().len();
    let pane_shows = format!("the prompt's pane, {pane} of the picture's {all} pixels");
    expect(&(pane * 100 >= all), &true, &pane_shows)?;
    let marks_show = format!("what's on the prompt's pane, {marks} pixels, {MARKS} at least");
    expect(&(marks >= MARKS), &true, &marks_show)?;
    Ok(format!(
        "the chat's picture, of a frame presented in two layers, has its prompt's pane, {pane} \
         pixels, and {marks} pixels of what's on it; saved {}",
        saved.display()
    ))
}

/// How many pixels of `picture` are of a pane filled with `fill`, and how many within the box
/// round those are of what's on it.
fn on_pane(picture: &RgbaImage, fill: [u8; 3]) -> (usize, usize) {
    let apart = |pixel: &image::Rgba<u8>| {
        let parts = pixel.0.iter().zip(fill);
        (parts.map(|(part, fill)| part.abs_diff(fill)).max()).unwrap_or(u8::MAX)
    };
    let of_pane: Vec<(u32, u32)> = (picture.enumerate_pixels())
        .filter(|(_, _, pixel)| apart(pixel) <= OF_PANE)
        .map(|(x, y, _)| (x, y))
        .collect();
    let (across, down) = (of_pane.iter().map(|at| at.0), of_pane.iter().map(|at| at.1));
    let ends = (
        across.clone().min(),
        across.max(),
        down.clone().min(),
        down.max(),
    );
    let (Some(left), Some(right), Some(top), Some(bottom)) = ends else {
        return (0, 0);
    };
    let marks = (picture.enumerate_pixels())
        .filter(|(x, y, _)| (left..=right).contains(x) && (top..=bottom).contains(y))
        .filter(|(_, _, pixel)| apart(pixel) >= ON_PANE)
        .count();
    (of_pane.len(), marks)
}

/// The picture of the tab shown as the app takes it: emptied, then taken of a frame the window
/// presents, and read once the GPU has drawn it.
fn taken(drawn: &mut Drawn) -> Result<RgbaImage> {
    let key = shown(drawn)?;
    // A tab shown for the first time has none to empty until the frame presented makes it.
    drawn.cx.read(|cx| pictures::blank(key, cx));
    present(drawn, key)?;
    let started = Instant::now();
    let mut last = None;
    loop {
        drawn.rest(SETTLES);
        let now = drawn.cx.read(|cx| pictures::image(key, cx));
        let now = now.ok_or("the tab has no picture")?;
        if last.as_ref() == Some(&now) && now.pixels().any(|pixel| pixel.0 != [0; 4]) {
            return Ok(now);
        }
        if started.elapsed() > PATIENCE {
            return Err("no picture was taken of the frame the window presented".into());
        }
        last = Some(now);
    }
}

/// The tab shown, by its first pane.
fn shown(drawn: &Drawn) -> Result<u64> {
    let key = drawn.cx.read(|cx| drawn.shell.read(cx).shown_tab_key());
    key.ok_or_else(|| "no tab is shown".into())
}

/// Has the shell's window draw a frame and present it as the app does, each of its layers its
/// own part of it, with the picture of the tab known by `key` asked for at once: what Core
/// Animation asks of the window's view when it wants a frame there and then.
fn present(drawn: &mut Drawn, key: u64) -> Result<()> {
    let view = (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            pictures::retake(key, cx);
            window.refresh();
            crate::native::window::gpui_view(window)
        })
        .map_err(|error| error.to_string())?;
    let view = view.ok_or("the shell's window has no view of macOS's")?;
    // SAFETY: `displayLayer:` is a layer's delegate's, which the view GPUI draws in is; GPUI's
    // takes no notice of the layer it's given.
    let _: () = unsafe { msg_send![&*view, displayLayer: std::ptr::null::<AnyObject>()] };
    Ok(())
}

/// Draws the shell's window as the renderer draws a frame, and reads it back: how long that
/// took. With `retaken`, the tab shown, its picture is asked for at once, so the frame takes
/// it; without, it's taken only when its turn has come, a quarter second after the last.
fn frame(drawn: &mut Drawn, retaken: Option<u64>) -> Result<Duration> {
    if let Some(shown) = retaken {
        drawn.cx.update(|cx| pictures::retake(shown, cx));
    }
    let started = Instant::now();
    (drawn.cx.capture_screenshot(drawn.window)).map_err(|error| error.to_string())?;
    Ok(started.elapsed())
}

/// What a tab's picture adds to a frame: a frame drawn and read back with its picture taken,
/// against one without, the middle of [`FRAMES`] of each. It's the most a picture costs, the
/// GPU waited for; in the app the frame isn't read back, and a picture is taken of four frames
/// a second at most.
fn cost(drawn: &mut Drawn) -> Result<String> {
    let shown = shown(drawn)?;
    let mut middle = |retaken: Option<u64>| -> Result<f64> {
        let mut took = (0..FRAMES)
            .map(|_| frame(drawn, retaken))
            .collect::<Result<Vec<_>>>()?;
        took.sort();
        Ok(took[took.len() / 2].as_secs_f64() * 1000.)
    };
    let (with, without) = (middle(Some(shown))?, middle(None)?);
    Ok(format!(
        "a frame drawn and read back took {with:.2} ms with the tab's picture taken, \
         {without:.2} ms without: {:.2} ms a picture (the middle of {FRAMES} frames each)",
        with - without
    ))
}

/// The tabs as the switcher lists them: the latest first.
fn titles() -> Vec<String> {
    (FILES.iter().rev())
        .filter_map(|file| file.rsplit('/').next())
        .chain(["New chat"])
        .map(String::from)
        .collect()
}

/// The switcher's cards' titles, its pick and whether its panel shows.
fn state(drawn: &Drawn) -> Result<(Vec<String>, usize, bool)> {
    (drawn.cx.read(super::state)).ok_or_else(|| "the switcher isn't up".into())
}

/// Whether the `panel`'s window shows, and whether it has the keyboard, as macOS says.
fn seen(drawn: &mut Drawn, panel: AnyWindowHandle) -> Result<(bool, bool)> {
    native(drawn, panel, |window| {
        (window.isVisible(), window.isKeyWindow())
    })
}

/// How the `panel`'s window is made: whether it has no edge of its own, and whether the glass is
/// behind what GPUI draws in it.
fn made(drawn: &mut Drawn, panel: AnyWindowHandle) -> Result<(bool, bool)> {
    native(drawn, panel, |window| {
        let borderless = !window.styleMask().contains(NSWindowStyleMask::Titled);
        (borderless, has_glass(window))
    })
}

/// Whether there's glass among the views of `window`.
fn has_glass(window: &NSWindow) -> bool {
    fn among(view: &NSView) -> bool {
        view.isKindOfClass(NSGlassEffectView::class())
            || (view.subviews().iter()).any(|inner| among(&inner))
    }
    window.contentView().is_some_and(|content| among(&content))
}

/// What `read` makes of the window macOS draws `window` in.
fn native<R>(
    drawn: &mut Drawn,
    window: AnyWindowHandle,
    read: impl FnOnce(&NSWindow) -> R,
) -> Result<R> {
    let read = (drawn.cx)
        .update_window(window, |_, window, _| {
            crate::native::window::ns_window(window).map(|window| read(&window))
        })
        .map_err(|error| error.to_string())?;
    read.ok_or_else(|| "GPUI's window has none of macOS's".into())
}

/// Fails, saying `what` was `found`, unless it's as `expected`.
fn expect<T: PartialEq + std::fmt::Debug>(found: &T, expected: &T, what: &str) -> Result<()> {
    if found == expected {
        Ok(())
    } else {
        Err(format!("{what}: {found:?}, not {expected:?}"))
    }
}
