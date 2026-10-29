//! Rust in the Files panel's viewer with the real rust-analyzer, the managed one fetched into
//! the data folder: the shell off screen on this repository's own checkout, `main.rs` shown
//! until its server is ready, then a hover, a definition in another file gone to with F12,
//! and a fixture crate's type error underlined. Saved as PNGs in `.superpowers/qa/lsp/`,
//! with how long each step took and how much memory the server holds once ready:
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/lsp/config COLOSSO_DATA_DIR=$PWD/.superpowers/data \
//!   cargo run -p colosso --features visual-check -- --visual-check-lsp
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use gpui_kit::component::input::EditorState;
use gpui_kit::{Entity, Modifiers, MouseButton};

use super::server::{Server, State};
use super::{Languages, install, managed};
use crate::files::{FileEditor, Place};
use crate::test_support::try_git;
use crate::visual::{Drawn, Result, checkout};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/lsp";
/// How long the server gets to load this workspace, building its build scripts first.
const LOADING: Duration = Duration::from_mins(20);
/// How long an answer gets.
const ANSWER: Duration = Duration::from_secs(60);
/// The fixture: a crate of its own, with a type error the server reports without building.
const FIXTURE_MANIFEST: &str = "[package]\nname = \"squiggle\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n# Not a member of the checkout's workspace around it.\n[workspace]\n";
const FIXTURE_MAIN: &str = "fn main() {\n    let greeting = \"héllo 🦀\";\n    let count: u32 = \"three\";\n    println!(\"{greeting} {count}\");\n}\n";

/// Draws the pictures and says where they are and what was measured, or why it couldn't.
/// The servers it started have stopped either way.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    if std::env::var_os("COLOSSO_DATA_DIR").is_none() {
        return Err("set COLOSSO_DATA_DIR, so the server is fetched into the check's".into());
    }
    let root = checkout()?;
    let fixture = fixture(&root.join(OUT))?;
    let mut drawn = Drawn::open_on_checkout(OUT, &[&fixture])?;
    drawn.cx.update(|cx| {
        // The managed server, not whatever PATH has.
        install(managed(false), cx);
        super::bind_keys(cx);
    });
    let mut lines = Vec::new();
    let shown = show(&mut drawn, &root, &fixture, &mut lines);
    let stopped = stop(&mut drawn);
    shown?;
    stopped?;
    Ok(lines)
}

/// The fixture crate under `out`, a repository of its own, made unless it's there.
fn fixture(out: &Path) -> Result<PathBuf> {
    let fixture = out.join("fixture");
    if !fixture.join(".git").exists() {
        std::fs::create_dir_all(fixture.join("src")).map_err(|error| error.to_string())?;
        std::fs::write(fixture.join("Cargo.toml"), FIXTURE_MANIFEST).map_err(|e| e.to_string())?;
        std::fs::write(fixture.join("src/main.rs"), FIXTURE_MAIN).map_err(|e| e.to_string())?;
        try_git(&fixture, &["init", "-q", "-b", "main"])?;
        try_git(&fixture, &["add", "."])?;
        try_git(&fixture, &["commit", "-q", "-m", "first"])?;
    }
    fixture.canonicalize().map_err(|error| error.to_string())
}

/// Each step in turn, saying what it saved and measured in `lines`.
fn show(drawn: &mut Drawn, root: &Path, fixture: &Path, lines: &mut Vec<String>) -> Result<()> {
    let editor = ready(drawn, root, lines)?;
    hover_and_go(drawn, root, &editor, lines)?;
    underlined(drawn, fixture, lines)
}

/// main.rs shown: its server starts, and is followed until it's ready. The editor showing it.
fn ready(drawn: &mut Drawn, root: &Path, lines: &mut Vec<String>) -> Result<Entity<EditorState>> {
    let started = Instant::now();
    let main = view(drawn, Path::new("crates/colosso/src/main.rs"))?;
    let repo = server(drawn, root)?;
    let mut indexing = None;
    drawn.until(LOADING, |drawn| {
        let state = drawn.cx.read(|cx| repo.read(cx).state().clone());
        match state {
            State::Ready => Ok(true),
            State::Failed(failure) => Err(format!("rust-analyzer: {}", failure.why)),
            State::Crashed(why) => Err(format!("rust-analyzer: {why}")),
            State::Stopped => Err("rust-analyzer stopped".into()),
            State::Indexing { total, .. } if total > 0 && indexing.is_none() => {
                indexing = Some(drawn.save("lsp-indexing.png")?);
                Ok(false)
            }
            State::Starting | State::Indexing { .. } => Ok(false),
        }
    })?;
    let ready = started.elapsed();
    lines.extend(indexing.map(|path| path.display().to_string()));
    lines.push(format!("ready after {:.1} s", ready.as_secs_f64()));
    lines.push(rss()?);
    let editor = editor_of(drawn, &main)?;
    drawn.pause()?;
    Ok(editor)
}

/// A hover over the type the app returns; then F12 on `shell::init`, which shows shell.rs
/// with the function picked.
fn hover_and_go(
    drawn: &mut Drawn,
    root: &Path,
    editor: &Entity<EditorState>,
    lines: &mut Vec<String>,
) -> Result<()> {
    let text = drawn.cx.read(|cx| editor.read(cx).text().to_string());
    let at = offset(&text, "use std::process::ExitCode;", "ExitCode")?;
    let asked = Instant::now();
    hover(drawn, editor, at)?;
    drawn.until(ANSWER, |drawn| {
        Ok(drawn
            .cx
            .read(|cx| editor.read(cx).hover_popover().is_some()))
    })?;
    lines.push(format!("hover shown in {} ms", asked.elapsed().as_millis()));
    drawn.pause()?;
    lines.push(drawn.save("lsp-hover.png")?.display().to_string());

    let at = offset(&text, "    shell::init(cx);", "init")? + 1;
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_selected_range(at..at, cx);
                editor.focus(window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()?;
    let target = root.join("crates/colosso/src/shell.rs");
    let asked = Instant::now();
    drawn.cx.simulate_keystrokes(drawn.window, "f12");
    drawn.until(ANSWER, |drawn| {
        let Some((path, view)) = viewing(drawn) else {
            return Ok(false);
        };
        if path != target {
            return Ok(false);
        }
        let picked = drawn.cx.read(|cx| {
            let editor = view.read(cx).editor()?.read(cx);
            let picked = editor.selected_text().to_string();
            Some(picked).filter(|picked| !picked.is_empty())
        });
        Ok(picked.is_some_and(|picked| picked == "init"))
    })?;
    lines.push(format!(
        "definition shown in {} ms",
        asked.elapsed().as_millis()
    ));
    drawn.pause()?;
    lines.push(drawn.save("lsp-definition.png")?.display().to_string());
    Ok(())
}

/// The fixture's type error, underlined, and its message on hover.
fn underlined(drawn: &mut Drawn, fixture: &Path, lines: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            if let Some(project) = shell.workspace.project_index(fixture) {
                shell.workspace.select_worktree(project, fixture);
            }
            cx.notify();
        });
    });
    drawn.draw()?;
    let started = Instant::now();
    let main = view(drawn, Path::new("src/main.rs"))?;
    server(drawn, fixture)?;
    drawn.until(LOADING, |drawn| {
        let Some(editor) = drawn.cx.read(|cx| main.read(cx).editor().cloned()) else {
            return Ok(false);
        };
        Ok(drawn.cx.read(|cx| {
            let diagnostics = editor.read(cx).diagnostics();
            diagnostics.is_some_and(|diagnostics| diagnostics.iter().next().is_some())
        }))
    })?;
    lines.push(format!(
        "fixture diagnostics after {:.1} s",
        started.elapsed().as_secs_f64()
    ));
    drawn.pause()?;
    lines.push(drawn.save("lsp-diagnostic.png")?.display().to_string());
    let editor = editor_of(drawn, &main)?;
    let at = offset(FIXTURE_MAIN, "\"three\"", "three")?;
    hover(drawn, &editor, at)?;
    drawn.until(ANSWER, |drawn| {
        Ok(drawn
            .cx
            .read(|cx| editor.read(cx).hover_popover().is_some()))
    })?;
    drawn.pause()?;
    lines.push(
        drawn
            .save("lsp-diagnostic-hover.png")?
            .display()
            .to_string(),
    );
    Ok(())
}

/// Shows the selected checkout's file at `path` in the Files panel, and its view.
fn view(drawn: &mut Drawn, path: &Path) -> Result<Entity<FileEditor>> {
    let shell = drawn.shell.clone();
    (drawn.cx)
        .update_window(drawn.window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.view_file(Place::file(path), window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()?;
    viewing(drawn)
        .map(|(_, view)| view)
        .ok_or_else(|| format!("{} isn't shown", path.display()))
}

/// The file the Files panel shows, and its view.
fn viewing(drawn: &Drawn) -> Option<(PathBuf, Entity<FileEditor>)> {
    drawn.cx.read(|cx| {
        let files = drawn.shell.read(cx).right_panel.files()?.read(cx);
        let viewed = files.viewing()?;
        Some((viewed.path.clone(), viewed.editor.clone()))
    })
}

/// The server for the checkout at `root`.
fn server(drawn: &Drawn, root: &Path) -> Result<Entity<Server>> {
    let server = drawn.cx.read(|cx| {
        let Languages(registry) = cx.global::<Languages>();
        let servers = registry.read(cx).servers();
        servers
            .into_iter()
            .find(|server| server.read(cx).root() == root)
    });
    server.ok_or_else(|| format!("no server started for {}", root.display()))
}

fn editor_of(drawn: &Drawn, view: &Entity<FileEditor>) -> Result<Entity<EditorState>> {
    let editor = drawn.cx.read(|cx| view.read(cx).editor().cloned());
    editor.ok_or_else(|| "the file isn't shown in an editor".into())
}

/// The byte offset of `needle` in the first `line` of `text` that has it.
fn offset(text: &str, line: &str, needle: &str) -> Result<usize> {
    let at = text.find(line).ok_or_else(|| format!("no `{line}`"))?;
    let inside = line
        .find(needle)
        .ok_or_else(|| format!("no `{needle}` in `{line}`"))?;
    Ok(at + inside)
}

/// Rests the pointer on the character at `at` in `editor`, long enough for it to ask.
fn hover(drawn: &mut Drawn, editor: &Entity<EditorState>, at: usize) -> Result<()> {
    drawn.draw()?;
    let bounds = drawn
        .cx
        .read(|cx| editor.read(cx).range_to_bounds(&(at..at + 1)));
    let bounds = bounds.ok_or("what's hovered isn't on screen")?;
    let window = drawn.window;
    let pointer = bounds.center();
    (drawn.cx).simulate_mouse_move(window, pointer, None::<MouseButton>, Modifiers::default());
    // The editor waits a moment before it asks.
    drawn.cx.advance_clock(Duration::from_millis(200));
    drawn.draw()
}

/// The memory each rust-analyzer run from the data folder holds.
fn rss() -> Result<String> {
    let data = crate::settings::data_dir().ok_or("no data folder")?;
    let binary = data.join("servers/rust-analyzer");
    let found = Command::new("pgrep")
        .arg("-f")
        .arg(binary.as_os_str())
        .output()
        .map_err(|error| error.to_string())?;
    let pids = String::from_utf8_lossy(&found.stdout).into_owned();
    let mut said = Vec::new();
    for pid in pids.split_whitespace() {
        let ps = Command::new("ps")
            .args(["-o", "rss=", "-p", pid])
            .output()
            .map_err(|error| error.to_string())?;
        let kilobytes = String::from_utf8_lossy(&ps.stdout).trim().to_owned();
        let megabytes = kilobytes.parse::<f64>().map_or(0., |kb| kb / 1024.);
        said.push(format!("pid {pid}: {megabytes:.0} MB"));
    }
    Ok(format!("rust-analyzer RSS when ready: {}", said.join(", ")))
}

/// Stops every server, and waits until each has exited.
fn stop(drawn: &mut Drawn) -> Result<()> {
    let stopping = drawn.cx.update(|cx| {
        let Languages(registry) = cx.global::<Languages>().clone();
        registry.update(cx, super::registry::Registry::stop_all)
    });
    let stopped = drawn.finish(ANSWER, stopping);
    stopped.map_err(|why| format!("the servers didn't stop: {why}"))
}
