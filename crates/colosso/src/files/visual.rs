//! The Files panel drawn as the app draws it, on this repository's own checkout, saved as PNGs
//! in `.superpowers/qa/files/`:
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/files/config \
//!   cargo run -p colosso --features visual-check -- --visual-check
//! ```

use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui_kit::component::input::RopeExt as _;
use gpui_kit::{Context, KeyUpEvent, Keystroke, PlatformInput};

use super::{FilesPanel, Place};
use crate::right_panel::{ShowFiles, ToggleChanges, ToggleWide};
use crate::visual::{Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/files";
/// How long the project gets to list what's asked of it.
const PATIENCE: Duration = Duration::from_secs(20);
/// How many keystrokes each way the tree's pick is timed over, as many as there are rows.
const STEPS: usize = 150;
/// The file shown beside the tree while the arrow keys are held: one of this repository's
/// longest, coloured as Rust.
const HELD_BESIDE: &str = "crates/colosso/src/changes/view.rs";
/// How long an arrow's held in the tree and in the file.
const HOLD: Duration = Duration::from_millis(1500);

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    drawn.cx.dispatch_action(drawn.window, ShowFiles);
    settle(&mut drawn, |panel| panel.scanned)?;

    // The arrow keys pressed in the tree, a long file shown beside it; then held, in the
    // tree and in the file.
    view(&mut drawn, HELD_BESIDE)?;
    drawn.pause()?;
    let mut timed = held(&mut drawn)?;
    timed.extend(repeated(&mut drawn)?);

    // A file shown beside the tree, beside the chat: its folders are revealed, and it's
    // picked.
    view(&mut drawn, "crates/project/src/lib.rs")?;
    toggle(&mut drawn, "script")?;
    drawn.pause()?;
    let tree = drawn.save("files-panel.png")?;

    // The same with the whole card, the chat out of sight.
    drawn.cx.dispatch_action(drawn.window, ToggleWide);
    drawn.pause()?;
    let wide = drawn.save("files-wide.png")?;
    drawn.cx.dispatch_action(drawn.window, ToggleWide);

    // An ignored folder opened: read once asked for, and shown dimmed.
    toggle(&mut drawn, "crates")?;
    toggle(&mut drawn, "target")?;
    settle(&mut drawn, |panel| {
        panel.rows.position("target/debug").is_some()
    })?;
    panel(&mut drawn, |panel, cx| {
        let at = panel.rows.position("target");
        at.map(|at| panel.select(at, cx))
    })?;
    let ignored = drawn.save("files-ignored.png")?;

    // The changes, opened from their own button in the top bar.
    drawn.cx.dispatch_action(drawn.window, ToggleChanges);
    drawn.pause()?;
    let changes = drawn.save("changes-switch.png")?;
    let saved = [tree, wide, ignored, changes];
    let mut lines: Vec<_> = saved
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    lines.extend(timed);
    Ok(lines)
}

/// The arrow keys held in the tree with a file shown beside it, down then up, as key repeat
/// sends them: each keystroke timed to its frame drawn (the event, then layout, prepaint and
/// paint), and what the frames built again rather than drew from the last.
fn held(drawn: &mut Drawn) -> Result<Vec<String>> {
    let focus = panel(drawn, |panel, _| panel.focus.clone())?;
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| window.focus(&focus, cx))
        .map_err(|error| error.to_string())?;
    panel(drawn, |panel, cx| panel.select(0, cx))?;
    drawn.draw()?;
    let rows = panel(drawn, |panel, _| panel.rows.len())?;
    let steps = STEPS.min(rows.saturating_sub(1));
    (drawn.cx)
        .update_window(window, |_, window, _| window.reset_layout_stats())
        .map_err(|error| error.to_string())?;
    let mut took = Vec::new();
    for key in ["down", "up"] {
        let keystroke = Keystroke::parse(key).map_err(|error| error.to_string())?;
        for _ in 0..steps {
            let started = Instant::now();
            // Two turns, as the event loop takes the key and then draws the frame.
            (drawn.cx)
                .update_window(window, |_, window, cx| {
                    window.dispatch_keystroke(keystroke.clone(), cx);
                })
                .map_err(|error| error.to_string())?;
            (drawn.cx)
                .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
                .map_err(|error| error.to_string())?;
            took.push(started.elapsed());
            // Let go before the next, as a tap is.
            let up = KeyUpEvent {
                keystroke: keystroke.clone(),
            };
            (drawn.cx)
                .update_window(window, |_, window, cx| {
                    window.dispatch_event(PlatformInput::KeyUp(up), cx);
                })
                .map_err(|error| error.to_string())?;
            drawn.cx.run_until_parked();
        }
        let at = panel(drawn, |panel, _| panel.cursor())?;
        let expected = if key == "down" { steps } else { 0 };
        if at != Some(expected) {
            return Err(format!(
                "{key} held left the pick at {at:?}, not {expected}"
            ));
        }
    }
    let stats = (drawn.cx)
        .update_window(window, |_, window, _| window.layout_stats())
        .map_err(|error| error.to_string())?;
    took.sort();
    let at = |share: f64| took[((took.len() - 1) as f64 * share).round() as usize];
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.;
    let frames = took.len() as f64;
    Ok(vec![
        format!(
            "tree step to frame over {} keystrokes: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            took.len(),
            ms(at(0.5)),
            ms(at(0.95)),
            ms(at(1.)),
        ),
        format!(
            "per frame: build {:.2} ms, prepaint {:.2} ms, paint {:.2} ms; views built {:.1}, reused {:.1}; lines shaped {:.1}",
            ms(stats.build_time) / frames,
            ms(stats.prepaint_time) / frames,
            ms(stats.paint_time) / frames,
            stats.views_built as f64 / frames,
            stats.views_reused as f64 / frames,
            stats.lines_shaped as f64 / frames,
        ),
    ])
}

/// The down arrow held in the tree, with some 300 rows open, and then in the file shown, which
/// has the keyboard.
fn repeated(drawn: &mut Drawn) -> Result<Vec<String>> {
    let folders = [
        "crates",
        "crates/colosso",
        "crates/colosso/src",
        "vendor",
        "vendor/gpui-base",
        "vendor/gpui-base/src",
        "vendor/gpui-component",
        "vendor/gpui-component/src",
    ];
    for folder in folders {
        settle(drawn, |panel| panel.rows.position(folder).is_some())?;
        let open = panel(drawn, |panel, _| {
            (panel.rows.position(folder)).is_some_and(|at| panel.rows.rows()[at].expanded)
        })?;
        if !open {
            toggle(drawn, folder)?;
        }
    }
    let last = "vendor/gpui-component/src/window_border.rs";
    settle(drawn, |panel| panel.rows.position(last).is_some())?;
    let rows = panel(drawn, |panel, cx| {
        panel.select(0, cx);
        panel.rows.len()
    })?;
    let pick = |drawn: &mut Drawn| panel(drawn, |panel, _| panel.cursor().unwrap_or(0));
    let tree = drawn.hold(&format!("tree of {rows} rows"), "down", HOLD, pick)?;
    let editor = panel(drawn, |panel, cx| {
        let viewed = panel.viewing()?;
        viewed.editor.read(cx).editor().cloned()
    })?;
    let editor = editor.ok_or("the file isn't shown in an editor")?;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(0..0, cx);
                editor.focus(window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    let line = |drawn: &mut Drawn| {
        Ok(drawn.cx.read(|cx| {
            let editor = editor.read(cx);
            editor.text().offset_to_position(editor.cursor()).line as usize
        }))
    };
    let file = drawn.hold("file", "down", HOLD, line)?;
    Ok(vec![tree, file])
}

/// Shows the checkout's file at `path` beside the tree.
fn view(drawn: &mut Drawn, path: &str) -> Result<()> {
    let (window, shell) = (drawn.window, drawn.shell.clone());
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.view_file(Place::file(path), window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()
}

/// Lets the Files panel take what the checkout's project publishes until it's `done`.
fn settle(drawn: &mut Drawn, done: impl Fn(&FilesPanel) -> bool) -> Result<()> {
    drawn.settle(PATIENCE, |drawn| panel(drawn, |panel, _| done(panel)))
}

fn panel<R>(
    drawn: &mut Drawn,
    f: impl FnOnce(&mut FilesPanel, &mut Context<FilesPanel>) -> R,
) -> Result<R> {
    let panel = (drawn.cx).read(|cx| drawn.shell.read(cx).right_panel.files().cloned());
    let panel = panel.ok_or("the Files panel isn't shown")?;
    Ok(drawn.cx.update(|cx| panel.update(cx, f)))
}

/// Opens or closes the folder at `path`, which is shown.
fn toggle(drawn: &mut Drawn, path: &str) -> Result<()> {
    let toggled = panel(drawn, |panel, cx| {
        let at = panel.rows.position(path)?;
        panel.toggle(at, cx);
        Some(())
    })?;
    toggled.ok_or_else(|| format!("{path} isn't shown"))?;
    drawn.draw()
}
