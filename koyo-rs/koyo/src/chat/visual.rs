//! A chat whose agent did some work, drawn as the app draws it at each verbosity, in the light
//! theme and the dark, and the Chat settings page with its Verbosity section, saved as PNGs in
//! `.superpowers/qa/chat/`. The fake provider does the work Claude would for a message that
//! starts "Show your work": it thinks, reads, searches, edits, runs the tests, writes a file and
//! updates its to-dos.
//!
//! The agent host's folder is short, as a socket's path can't be long.
//!
//! ```sh
//! mkdir -p .superpowers/c/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$PWD/.superpowers/c TMPDIR=$PWD/.superpowers/c/tmp \
//!   KOYO_CONFIG_DIR=$PWD/.superpowers/qa/chat/config KOYO_LOG_DIR=$PWD/.superpowers/c/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-chat
//! ```

use std::process::ExitCode;
use std::time::{Duration, Instant};

use crate::navigation::Route;
use crate::settings::{self, Preset};
use crate::settings_view::SettingsPage;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/chat";
/// How much longer than it waits for a wait may go on, with the app slow to draw.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the chat gets to take in what the fake provider did, and colour its diffs.
const ANSWERED_IN: Duration = Duration::from_secs(4);
/// What's told to the chat: the fake provider's cue to do some work first.
const SAID: &str = "Show your work: the last line of a file goes missing.";

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let shell = drawn.shell.clone();
    drawn.chat_saying(&checkout()?, SAID)?;
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.chat_verbosity = Preset::Normal.verbosity();
        });
    });
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| {
        Ok(start.elapsed() >= ANSWERED_IN)
    })?;

    let mut saved = Vec::new();
    for preset in Preset::ALL {
        let verbosity = preset.verbosity();
        drawn
            .cx
            .update(|cx| settings::update(cx, |settings| settings.chat_verbosity = verbosity));
        saved.push(drawn.save(&format!("chat-{}.png", preset.label().to_lowercase()))?);
    }
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.chat_verbosity = Preset::Normal.verbosity();
        });
        cx.dispatch_action(&UseDarkTheme);
    });
    saved.push(drawn.save("chat-normal-dark.png")?);
    drawn.cx.update(|cx| {
        cx.dispatch_action(&UseLightTheme);
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::Chat), cx);
        });
    });
    saved.push(drawn.save("settings-chat.png")?);
    Ok((saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect())
}
