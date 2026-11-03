//! A terminal tab drawn as the app draws it, on a shell the daemon runs in a scratch
//! repository, saved as PNGs in `.superpowers/qa/terminal/`: the prompt, a command and its
//! output, history and Ctrl-C, text through the input method, the status and control bar as
//! an agent holds the terminal and the person takes it, colours and box drawing, a
//! selection by mouse, the history scrolled back, a full-screen program, the tab narrowed by
//! the right panel, and the tab closed with ⌘W and opened again with ⌘⇧T, then brought back
//! from the daemon as the app does when it starts, after the daemon restarted, and from
//! under a daemon of another build. It says how long the prompt and a long output took to
//! show.
//!
//! The daemon, its shell and what they write stay in the checkout: the shell's home is a
//! scratch folder, and the daemon's folder is short, as a socket's path can't be long. The
//! check starts the daemon afresh in it, so no earlier run's terminals come back.
//!
//! ```sh
//! mkdir -p .superpowers/t/home .superpowers/t/tmp && touch .superpowers/t/home/.zshrc &&
//!   HOME=$PWD/.superpowers/t/home TMPDIR=$PWD/.superpowers/t/tmp \
//!   KOYO_DATA_DIR=$PWD/.superpowers/t KOYO_CONFIG_DIR=$PWD/.superpowers/qa/terminal/config \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-terminal
//! ```

mod control;
mod drive;
mod delete;
mod panel;
mod paste;
mod stop;
mod upgrade;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use koyo_term::{ScrollPageUp, ScrollToBottom, TermClient};
use koyo_term_control::{
    ClientKind, DaemonCommand, Hello, TermDir,
};

use crate::projects::read_worktrees;
use crate::shell::{NewTerminal, Shell, ToggleRightPanel};
use crate::test_support::try_git;
use crate::visual::{Drawn, Result, checkout};
use drive::{
    act, alt_screen, cell, click, drag, focused, press, selected, terminal, text, type_keys,
    type_line, wait, wait_until, wheel,
};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/terminal";
/// Colours, styles and box drawing, pasted as one command.
const COLOURS: &str = "printf '\\e[1;31mbold red\\e[0m \\e[3;32mitalic green\\e[0m \\e[4;34munderline blue\\e[0m \\e[7m inverse \\e[0m \\e[48;5;214m\\e[30m 256 \\e[0m \\e[38;2;120;80;220mtruecolor\\e[0m\\n┌──┬──┐ ━━┳━━ ▀▄█▌▐░▒▓ 字 🙂\\n│ab│cd│   ┃   ╭─╮ ═══\\n└──┴──┘ ━━┻━━ ╰─╯ box-done\\n'";

pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let data = std::env::var_os("KOYO_DATA_DIR")
        .map(PathBuf::from)
        .ok_or("set KOYO_DATA_DIR, so the check's daemon stays out of the app's")?;
    // The folder is cleared, so it must be the check's own.
    if !data.starts_with(checkout()?) {
        return Err("KOYO_DATA_DIR must be a folder in this checkout".into());
    }
    let term = data.join("term");
    let _ = std::fs::remove_dir_all(&term);
    let repo = scratch_repository()?;
    let mut drawn = Drawn::open_in(OUT)?;
    let shell = drawn.shell.clone();
    let selected = drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            let project = shell.workspace.add_project(repo.clone());
            shell.show_worktrees(&repo, read_worktrees(&repo), cx);
            shell.workspace.select_worktree(project, &repo)
        })
    });
    if !selected {
        return Err("the scratch repository wasn't listed".into());
    }
    let mut said = Vec::new();
    let found = run_shell(&mut drawn, &mut said);
    // The check's daemon goes with it, whatever happened, stopped through the app's own
    // client so the views that lose it don't start it again.
    let client = drawn.cx.update(|cx| shell.update(cx, |shell, _| shell.terminal_client()));
    let client = client
        .unwrap_or_else(|| TermClient::new(TermDir::at(term.clone()), DaemonCommand::binary("koyo")));
    if let Err(error) = client.shutdown() {
        said.push(format!("couldn't stop the check's daemon: {error}"));
    }
    // It takes its socket away last, once its shells are gone.
    let socket = TermDir::at(term).socket();
    if let Err(error) = drawn.until(Duration::from_secs(10), |_| Ok(!socket.exists())) {
        said.push(format!("the check's daemon didn't stop: {error}"));
    }
    found.map(|()| said)
}

fn run_shell(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let started = Instant::now();
    act(drawn, Box::new(NewTerminal))?;
    wait(drawn, "the prompt", |text| !text.trim().is_empty())?;
    said.push(format!(
        "the prompt was drawn {} ms after New Terminal",
        started.elapsed().as_millis()
    ));
    let had = focused(drawn)?;
    said.push(format!("the new terminal had the keyboard on its own: {had}"));

    type_line(drawn, "echo hello-koyo")?;
    wait(drawn, "echo's output", |text| text.matches("hello-koyo").count() >= 2)?;
    said.push(format!("{}", drawn.save("shell.png")?.display()));
    keys(drawn)?;
    paste::paste(drawn, said)?;
    control::control(drawn, said)?;
    panel::panel(drawn, said)?;
    stop::stop(drawn, said)?;
    delete::delete(drawn, said)?;

    // Held no longer than this: the check later waits for the tab's view to go.
    let view = terminal(drawn)?;
    (drawn.cx).update(|cx| view.update(cx, |view, _| view.paste_text(COLOURS.into())));
    drop(view);
    press(drawn, "enter")?;
    wait(drawn, "the colours pasted", |text| {
        text.lines().any(|line| line.ends_with("box-done"))
    })?;
    said.push(format!("{}", drawn.save("colours.png")?.display()));

    selection(drawn, said)?;
    scrollback(drawn, said)?;
    full_screen(drawn, said)?;
    resized(drawn, said)?;
    reattached(drawn, said)?;
    after_restart(drawn, said)?;
    upgrade::other_build(drawn, said)?;
    stop::end_everything(drawn, said)
}

/// The shell's history and Ctrl-C, and text the input method delivers.
fn keys(drawn: &mut Drawn) -> Result<()> {
    press(drawn, "up")?;
    press(drawn, "enter")?;
    wait(drawn, "the command again from history", |text| {
        text.matches("hello-koyo").count() >= 4
    })?;
    type_line(drawn, "sleep 30")?;
    wait(drawn, "sleep to start", |text| {
        text.ends_with("sleep 30") || text.ends_with("sleep 30\n")
    })?;
    // Pressed the instant Enter is, Ctrl-C reaches the shell before its job holds the
    // terminal; a person is never that quick.
    let started = Instant::now();
    wait_until(drawn, "a moment", |_| Ok(started.elapsed() >= Duration::from_millis(300)))?;
    press(drawn, "ctrl-c")?;
    type_line(drawn, "echo interrupted")?;
    wait(drawn, "a command after Ctrl-C", |text| {
        text.lines().any(|line| line == "interrupted")
    })?;
    type_line(drawn, "echo café-ü")?;
    wait(drawn, "accented text", |text| text.lines().any(|line| line == "café-ü"))?;
    program_keys(drawn)
}

/// Keys the app would take if the terminal didn't keep them: Tab reaches the shell, which
/// completes the command, and ⌘⌫ deletes the line, as `MAC_EDITING` has it.
fn program_keys(drawn: &mut Drawn) -> Result<()> {
    type_keys(drawn, "whoam")?;
    wait(drawn, "the start of a command", |text| text.trim_end().ends_with("whoam"))?;
    press(drawn, "tab")?;
    wait(drawn, "Tab's completion", |text| text.trim_end().ends_with("whoami"))?;
    press(drawn, "cmd-backspace")?;
    wait(drawn, "⌘⌫ to delete the line", |text| !text.trim_end().ends_with("whoami"))?;
    type_line(drawn, "echo keys-reached")?;
    wait(drawn, "a command on the emptied line", |text| {
        text.lines().any(|line| line == "keys-reached")
    })
}

/// A word chosen by a double click and a run of cells by a drag, read back as Copy reads
/// them.
fn selection(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let row = (text(drawn)?.lines()).position(|line| line == "interrupted");
    let row = u16::try_from(row.ok_or("the word to select isn't on screen")?)
        .map_err(|error| error.to_string())?;
    let word = cell(drawn, 3, row)?;
    click(drawn, word, 2)?;
    let double = selected(drawn)?;
    if double.as_deref() != Some("interrupted") {
        return Err(format!("a double click selected {double:?}, not the word"));
    }
    let (from, to) = (cell(drawn, 0, row)?, cell(drawn, 4, row)?);
    drag(drawn, from, to)?;
    let dragged = selected(drawn)?;
    if dragged.as_deref() != Some("inter") {
        return Err(format!("dragging over five cells selected {dragged:?}"));
    }
    said.push(format!("{}", drawn.save("selection.png")?.display()));
    Ok(())
}

/// A long output, how long it took to show, and its history paged and wheeled back.
fn scrollback(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let started = Instant::now();
    type_line(drawn, "seq 1 100000")?;
    wait(drawn, "seq's last line", |text| text.lines().any(|line| line == "100000"))?;
    said.push(format!(
        "seq 1 100000 was drawn to its end {} ms after Enter",
        started.elapsed().as_millis()
    ));
    act(drawn, Box::new(ScrollPageUp))?;
    wait(drawn, "a page of history", |text| {
        !text.contains("100000") && text.lines().any(|line| line == "99950")
    })?;
    said.push(format!("{}", drawn.save("scrolled.png")?.display()));
    act(drawn, Box::new(ScrollToBottom))?;
    wait(drawn, "the bottom again", |text| text.lines().any(|line| line == "100000"))?;
    let at = cell(drawn, 10, 10)?;
    wheel(drawn, at, 5.)?;
    wait(drawn, "the wheel's history", |text| {
        !text.contains("100000") && text.lines().any(|line| line == "99995")
    })?;
    act(drawn, Box::new(ScrollToBottom))?;
    wait(drawn, "the bottom after the wheel", |text| text.contains("100000"))
}

/// A program on the alternate screen, and the shell's screen back after it.
fn full_screen(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    type_line(drawn, "vi -u NONE README.md")?;
    wait_until(drawn, "vi", |drawn| {
        Ok(alt_screen(drawn)? && text(drawn)?.contains("scratch"))
    })?;
    said.push(format!("{}", drawn.save("vi.png")?.display()));
    type_line(drawn, ":q!")?;
    wait_until(drawn, "vi quitting", |drawn| Ok(!alt_screen(drawn)?))?;
    said.push(format!("{}", drawn.save("after-vi.png")?.display()));
    Ok(())
}

/// The tab narrowed by the right panel opening beside it, as its shell reads it.
fn resized(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let view = terminal(drawn)?;
    let size = |drawn: &mut Drawn| drawn.cx.read(|cx| view.read(cx).grid_size());
    let Some((cols_before, _)) = size(drawn) else {
        return Err("the tab's terminal isn't laid out".into());
    };
    act(drawn, Box::new(ToggleRightPanel))?;
    wait_until(drawn, "the tab to narrow", |drawn| {
        Ok(size(drawn).is_some_and(|(cols, _)| cols < cols_before))
    })?;
    // The size settles before the daemon hears it.
    drawn.pause()?;
    let (cols, rows) = size(drawn).ok_or("the tab's terminal isn't laid out")?;
    focused(drawn)?;
    type_line(drawn, "stty size")?;
    let expected = format!("{rows} {cols}");
    wait(drawn, "the shell's size", |text| text.lines().any(|line| line == expected))?;
    said.push(format!(
        "the right panel took the tab from {cols_before} columns to {cols}, as its shell reads"
    ));
    said.push(format!("{}", drawn.save("narrowed.png")?.display()));
    Ok(())
}

/// The tab gone as quitting the app takes it, which leaves the terminal running in the daemon,
/// and brought back from the daemon as the app does when it starts, its screen as it was.
fn reattached(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    // The tab is narrow by now, so the command may have wrapped.
    let as_it_was = |text: &str| text.replace('\n', "").contains("% stty size");
    let closed = terminal(drawn)?.downgrade();
    let shell = drawn.shell.clone();
    (drawn.cx).update(|cx| shell.update(cx, Shell::let_go_of_shown_terminal));
    wait_until(drawn, "the tab to go", |drawn| Ok(terminal(drawn).is_err()))?;
    // Its view goes with it, and lets the terminal go in the daemon.
    wait_until(drawn, "its view to go", |_| Ok(closed.upgrade().is_none()))?;
    (drawn.cx).update(|cx| shell.update(cx, Shell::restore_terminals_again));
    wait(drawn, "the terminal back from the daemon", as_it_was)?;
    said.push(format!("{}", drawn.save("restored.png")?.display()));
    Ok(())
}

/// The daemon stopped under the app, as a reboot stops it, and the terminals brought back as
/// the app does when it starts: the person's shell runs again from the screen it had.
fn after_restart(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    const RESTORED: &str = "[restored after the terminal daemon restarted]";
    let shell = drawn.shell.clone();
    let closed = terminal(drawn)?.downgrade();
    (drawn.cx).update(|cx| shell.update(cx, Shell::let_go_of_shown_terminal));
    wait_until(drawn, "the tab to go", |drawn| Ok(terminal(drawn).is_err()))?;
    wait_until(drawn, "its view to go", |_| Ok(closed.upgrade().is_none()))?;
    let client = drawn.cx.update(|cx| shell.update(cx, |shell, _| shell.terminal_client()));
    let dir = client.ok_or("the app has no terminal client")?.dir().clone();
    stop_daemon(&dir)?;
    wait_until(drawn, "the daemon to stop", |_| Ok(!dir.socket().exists()))?;
    (drawn.cx).update(|cx| shell.update(cx, Shell::restore_terminals_again));
    // The tab is narrow by now, so lines wrap.
    wait(drawn, "the shell run again from its screen", |text| {
        let text = text.replace('\n', "");
        let (before, after) = text.split_once(RESTORED).unwrap_or_default();
        before.contains("% stty size") && after.contains('%')
    })?;
    control::holds(drawn, "the shell run again to be at its prompt", |state| {
        state.status == koyo_term_control::Status::Idle && state.reason == "prompt"
    })?;
    said.push(format!("{}", drawn.save("after-restart.png")?.display()));
    said.push("after the daemon restarted, the shell ran again from the screen it had".into());
    Ok(())
}

/// Stops the daemon as the command line does; the app's own client wouldn't start it again.
fn stop_daemon(dir: &TermDir) -> Result<()> {
    let hello = Hello::new(ClientKind::Cli, "visual-check", "0");
    koyo_term_control::client::stop_daemon(dir, &hello)
        .map_err(|error| format!("the daemon didn't stop: {error}"))
}

/// A repository of one commit under the check's folder, made afresh.
fn scratch_repository() -> Result<PathBuf> {
    let root = checkout()?.join(OUT).join("repo");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    std::fs::write(
        root.join("README.md"),
        "A scratch repository for the terminal check.\n",
    )
    .map_err(|error| error.to_string())?;
    try_git(&root, &["init", "-q", "-b", "main"])?;
    try_git(&root, &["add", "."])?;
    try_git(&root, &["commit", "-q", "-m", "scratch"])?;
    root.canonicalize().map_err(|error| error.to_string())
}
