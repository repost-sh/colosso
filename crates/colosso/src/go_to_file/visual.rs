//! Go to File drawn as the app draws it: the real shell off screen on this repository's own
//! checkout with a file open in the Files viewer, ⌘P typed into it, the arrow keys held over
//! what it lists, a query typed and Enter pressed, saved as PNGs in `.superpowers/qa/finder/`.
//! The palette is a window of its own, glass the system blurs; the pictures of it over the
//! shell paste it onto the shell's, with the shell blurred where it lies, which is what the
//! system shows.
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/finder/config \
//!   cargo run -p colosso --features visual-check -- --visual-check-finder
//! ```

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_project::{MOST_FOUND, Project};
use gpui_kit::{AnyWindowHandle, App, Entity, Keystroke, Window};
use image::{RgbaImage, imageops};

use super::view::GoToFile;
use crate::files::Place;
use crate::modal;
use crate::visual::{self, Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/finder";
/// How long the project gets to list the checkout, and the palette to search it.
const PATIENCE: Duration = Duration::from_secs(30);
/// The file the Files viewer shows behind the palette.
const BEHIND: &str = "crates/colosso/src/shell.rs";
/// How many times each arrow key is pressed while timed.
const STEPS: usize = 60;

/// Draws the pictures and says where they are and what was timed, or why it couldn't.
pub fn run() -> ExitCode {
    visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    drawn.until(PATIENCE, |drawn| Ok(project(drawn).is_some()))?;
    drawn.settle(PATIENCE, |drawn| {
        Ok(project(drawn).is_some_and(|project| project.scanned().is_some()))
    })?;
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    on(&mut drawn, window, |window, cx| {
        shell.update(cx, |shell, cx| {
            shell.view_file(Place::file(BEHIND), window, cx);
        });
    })?;
    drawn.pause()?;

    // ⌘P as typed, to the palette's first frame.
    let started = Instant::now();
    drawn.cx.simulate_keystrokes(drawn.window, "cmd-p");
    let palette = palette_window(&mut drawn)?;
    frame(&mut drawn, palette)?;
    let mut lines = vec![format!(
        "first frame of the palette {:.1} ms after cmd-p",
        ms(started.elapsed())
    )];
    shown(&mut drawn, palette, "", |palette| {
        palette.results.found.len() == MOST_FOUND
    })?;
    lines.extend(held(&mut drawn, palette)?);

    drawn.cx.simulate_input(palette, "chv");
    lines.push(expect(
        &mut drawn,
        palette,
        "chv",
        "crates/colosso/src/changes/view.rs",
    )?);
    lines.push(save_over(&mut drawn, palette, "finder-chv.png")?);

    drawn.cx.simulate_keystrokes(palette, "cmd-a backspace");
    drawn.cx.simulate_input(palette, "finderrank:40");
    lines.push(expect(
        &mut drawn,
        palette,
        "finderrank",
        "crates/project/src/finder/rank.rs",
    )?);
    lines.push(save_over(&mut drawn, palette, "finder-line.png")?);

    // Enter shows the file picked in the Files panel behind, at the line typed.
    drawn.cx.simulate_keystrokes(palette, "enter");
    drawn.pause()?;
    let viewed = drawn.cx.read(|cx| {
        let files = drawn.shell.read(cx).right_panel.files().cloned()?;
        Some(files.read(cx).viewing()?.path.clone())
    });
    lines.push(format!("shown after enter: {viewed:?}"));
    lines.push(drawn.save("finder-opened.png")?.display().to_string());
    Ok(lines)
}

/// The selected checkout's project, once it's open.
fn project(drawn: &Drawn) -> Option<Arc<Project>> {
    drawn.cx.read(|cx| {
        let checkout = drawn.shell.read(cx).right_panel.checkout()?;
        checkout.read(cx).project().cloned()
    })
}

/// The palette on screen.
fn palette(drawn: &Drawn) -> Option<Entity<GoToFile>> {
    drawn.cx.read(modal::current::<GoToFile>)
}

/// The window ⌘P opened over the shell's.
fn palette_window(drawn: &mut Drawn) -> Result<AnyWindowHandle> {
    let shell = drawn.window;
    let windows = drawn.cx.update(|cx| cx.windows());
    (windows.into_iter())
        .find(|window| *window != shell)
        .ok_or_else(|| "cmd-p opened no palette".into())
}

fn on<R>(
    drawn: &mut Drawn,
    window: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> Result<R> {
    (drawn.cx)
        .update_window(window, |_, window, cx| f(window, cx))
        .map_err(|error| error.to_string())
}

fn frame(drawn: &mut Drawn, palette: AnyWindowHandle) -> Result<()> {
    drawn.cx.run_until_parked();
    on(drawn, palette, |window, cx| window.draw(cx).clear(cx))
}

/// Lets the palette take up the checkout's files and search for `typed` until `done` says
/// what it shows is what was waited for.
fn shown(
    drawn: &mut Drawn,
    palette: AnyWindowHandle,
    typed: &str,
    done: impl Fn(&GoToFile) -> bool,
) -> Result<()> {
    drawn.until(PATIENCE, |drawn| {
        frame(drawn, palette)?;
        let shown = self::palette(drawn).is_some_and(|shown| {
            drawn.cx.read(|cx| {
                let shown = shown.read(cx);
                shown.listed && shown.results.query.text == typed && done(shown)
            })
        });
        Ok(shown)
    })
}

/// Waits until the palette shows what `typed` finds, and its first is `first`; says what it
/// found.
fn expect(drawn: &mut Drawn, palette: AnyWindowHandle, typed: &str, first: &str) -> Result<String> {
    shown(drawn, palette, typed, |_| true)?;
    let shown = self::palette(drawn).ok_or("the palette closed")?;
    let (paths, matched, files) = drawn.cx.read(|cx| {
        let shown = shown.read(cx);
        let paths: Vec<String> = (shown.results.found.iter().take(3))
            .map(|found| found.path.to_string())
            .collect();
        (paths, shown.results.matched, shown.results.files)
    });
    if paths.first().map(String::as_str) != Some(first) {
        return Err(format!("{typed:?} found {paths:?} first, not {first}"));
    }
    Ok(format!(
        "{typed:?}: {matched} of {files} files match; first {paths:?}"
    ))
}

/// The arrow keys held over what the palette lists with nothing typed, down then up, as key
/// repeat sends them: each keystroke timed to the palette's frame drawn (the event, then
/// layout, prepaint and paint), and what its frames built again rather than drew from the
/// last. The window behind is drawn after each, timed apart, for whether a step touched it.
fn held(drawn: &mut Drawn, palette: AnyWindowHandle) -> Result<Vec<String>> {
    let shown = self::palette(drawn).ok_or("the palette closed")?;
    let found = |drawn: &Drawn| drawn.cx.read(|cx| shown.read(cx).results.found.as_ptr());
    let before = found(drawn);
    let behind = drawn.window;
    // The window behind drawn with nothing pressed, for what drawing it builds anyway.
    on(drawn, behind, |window, _| window.reset_layout_stats())?;
    for _ in 0..2 * STEPS {
        on(drawn, behind, |window, cx| window.draw(cx).clear(cx))?;
    }
    let idle = per_frame(drawn, behind, "window behind, nothing pressed,")?;
    for window in [palette, behind] {
        on(drawn, window, |window, _| window.reset_layout_stats())?;
    }
    let (mut took, mut redrawn) = (Vec::new(), Vec::new());
    for (key, expected) in [("down", STEPS), ("up", 0)] {
        let keystroke = Keystroke::parse(key).map_err(|error| error.to_string())?;
        for _ in 0..STEPS {
            let started = Instant::now();
            // Two turns, as the event loop takes the key and then draws the frame.
            on(drawn, palette, |window, cx| {
                window.dispatch_keystroke(keystroke.clone(), cx);
            })?;
            on(drawn, palette, |window, cx| window.draw(cx).clear(cx))?;
            took.push(started.elapsed());
            let started = Instant::now();
            on(drawn, behind, |window, cx| window.draw(cx).clear(cx))?;
            redrawn.push(started.elapsed());
            drawn.cx.run_until_parked();
        }
        let at = drawn.cx.read(|cx| shown.read(cx).selected);
        if at != expected {
            return Err(format!("{key} held left the pick at {at}, not {expected}"));
        }
    }
    if found(drawn) != before {
        return Err("moving the pick searched again".into());
    }
    let listed = drawn.cx.read(|cx| shown.read(cx).results.found.len());
    let mut lines = vec![timed(
        &format!("palette step to frame over {listed} files listed"),
        &mut took,
    )];
    lines.push(per_frame(drawn, palette, "palette")?);
    lines.push(timed("the window behind drawn after each", &mut redrawn));
    lines.push(per_frame(drawn, behind, "window behind")?);
    lines.push(idle);
    Ok(lines)
}

/// p50, p95 and the longest of `took`.
fn timed(what: &str, took: &mut [Duration]) -> String {
    took.sort();
    let at = |share: f64| took[((took.len() - 1) as f64 * share).round() as usize];
    format!(
        "{what}, {} keystrokes: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        took.len(),
        ms(at(0.5)),
        ms(at(0.95)),
        ms(at(1.)),
    )
}

/// What `window`'s frames since its stats were reset built, on average.
fn per_frame(drawn: &mut Drawn, window: AnyWindowHandle, name: &str) -> Result<String> {
    let stats = on(drawn, window, |window, _| window.layout_stats())?;
    let frames = (2 * STEPS) as f64;
    Ok(format!(
        "{name} per frame: build {:.3} ms, prepaint {:.3} ms, paint {:.3} ms; views built {:.1}, reused {:.1}; lines shaped {:.1}",
        ms(stats.build_time) / frames,
        ms(stats.prepaint_time) / frames,
        ms(stats.paint_time) / frames,
        stats.views_built as f64 / frames,
        stats.views_reused as f64 / frames,
        stats.lines_shaped as f64 / frames,
    ))
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}

/// Saves the shell with the palette over it, the shell blurred under it as the system blurs
/// what's behind a glass window.
fn save_over(drawn: &mut Drawn, palette: AnyWindowHandle, name: &str) -> Result<String> {
    drawn.draw()?;
    frame(drawn, palette)?;
    let bounds = |window: AnyWindowHandle, drawn: &mut Drawn| {
        on(drawn, window, |window, _| {
            (window.bounds(), window.scale_factor())
        })
    };
    let (shell_bounds, scale) = bounds(drawn.window, drawn)?;
    let (palette_bounds, _) = bounds(palette, drawn)?;
    let capture = |window: AnyWindowHandle, drawn: &mut Drawn| {
        (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())
    };
    let mut picture: RgbaImage = capture(drawn.window, drawn)?;
    let over: RgbaImage = capture(palette, drawn)?;
    // Centred across the shell as the dialog opens: off screen, the windows' widths aren't
    // reported as they're drawn, so only the drop from the top is taken from their bounds.
    let below = palette_bounds.origin.y - shell_bounds.origin.y;
    let x = picture.width().saturating_sub(over.width()) / 2;
    let y = (below.as_f32() * scale) as u32;
    let under = imageops::crop_imm(&picture, x, y, over.width(), over.height()).to_image();
    imageops::replace(
        &mut picture,
        &imageops::blur(&under, 12. * scale),
        x.into(),
        y.into(),
    );
    imageops::overlay(&mut picture, &over, x.into(), y.into());
    let path = visual::checkout()?.join(OUT).join(name);
    picture.save(&path).map_err(|error| error.to_string())?;
    Ok(path.display().to_string())
}
