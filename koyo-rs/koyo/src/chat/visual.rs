//! Two checks of how a chat is drawn, saved as PNGs under `.artifacts/qa/`.
//!
//! `--visual-check-chat`: a chat whose agent did some work, drawn as the app draws it at each
//! verbosity, in the light theme and the dark, and the Chat settings page with its Verbosity
//! section, in `.artifacts/qa/chat/`. The fake provider does the work Claude would for a message
//! that starts "Show your work": it thinks, reads, searches, edits, runs the tests, writes a file
//! and updates its to-dos. Last, a chat whose agent keeps a task running in the background, and the
//! dialog quitting opens then, which names it; its Stop Everything stops the agent host, which
//! nothing starts again.
//!
//! `--visual-check-chat-theme`: a chat in a gradient theme of a colour, blue going to violet, whose
//! answer holds what markdown sets apart from its text (a table, code inline and in a block, a
//! quote), dark and light, in `.artifacts/qa/chat-theme/`: whether what's set apart takes the
//! theme's colour and lets the card show through. The fake provider says back what it's told, so
//! the person's message is the markdown and the answer draws it.
//!
//! `--visual-check-chat-markdown`: markdown in a chat's messages and composer, its floating
//! prompts and a picture in the viewer, in `.artifacts/qa/chat-markdown/` (`visual/markdown.rs`).
//!
//! `--visual-check-chat-progress`: the plan a chat's agent keeps, in its details card, in
//! `.artifacts/qa/chat-progress/` (`visual/progress.rs`).
//!
//! Each check keeps its settings in its own folder, as the theme check changes the gradient theme:
//! the theme check's is `.artifacts/qa/chat-theme/config`.
//!
//! ```sh
//! mkdir -p .artifacts/c/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$PWD/.artifacts/c TMPDIR=$PWD/.artifacts/c/tmp \
//!   KOYO_CONFIG_DIR=$PWD/.artifacts/qa/chat/config KOYO_LOG_DIR=$PWD/.artifacts/c/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-chat
//! ```

mod markdown;
mod progress;

use std::time::{Duration, Instant};

use gpui_kit::{Action, point};

use crate::gradient::{DotKind, GradientTheme, Harmony};
use crate::modal;
use crate::quitting::{Quit, QuitDialog};
use crate::settings::{self, Preset};
use crate::navigation::SettingsPage;
use crate::shell::Shell;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".artifacts/qa/chat";
const THEME_OUT: &str = ".artifacts/qa/chat-theme";
/// How much longer than it waits for a wait may go on, with the app slow to draw.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the chat gets to take in what the fake provider did, and colour its diffs.
const ANSWERED_IN: Duration = Duration::from_secs(4);
/// What's told to the chat: the fake provider's cue to do some work first.
const SAID: &str = "Show your work: the last line of a file goes missing.";
/// What's told to another: the fake provider's cue to leave a task running in the background.
const IN_BACKGROUND: &str = "Keep working in the background: watch the build.";
/// How long the stopped host is watched for anything starting it again: past the chats' first
/// wait to reach it once more.
const STAYS_STOPPED: Duration = Duration::from_secs(3);
/// What's told to the chat in the theme check, and so said back: what an answer about a design
/// holds.
const SAID_IN_MARKDOWN: &str = "\
Here's how the topics are laid out.

## Topology

| Topic | Who publishes | Who consumes |
|---|---|---|
| `external.common` (partitioned, 32) | router, retry-scheduler, transformer | \
`forwarder-external`, one Shared subscription |
| `external.rate-limited.<id>` | the same three, for rate-limited forwarders | \
`forwarder-external-rate-limited`, Key_Shared |
| `replay.external.<job>` | replay-manager | `forwarder-external-rate-limited` |

Today's subscription pattern is `external\\..*` (topic.go:46). Confirm it against production \
before the migration:

```sql
SELECT count(*) FROM forwarder WHERE id !~ '^c[a-z0-9]{24}$';
```

> Go's regexp has no negative lookahead, so this is the way to exclude `common`.";

/// What draws the pictures of the chat check the command line's `arg` names, if it names one.
pub fn check(arg: &str) -> Option<fn() -> Result<Vec<String>>> {
    match arg {
        "--visual-check-chat" => Some(draw),
        "--visual-check-chat-theme" => Some(draw_theme),
        "--visual-check-chat-markdown" => Some(markdown::draw),
        "--visual-check-chat-progress" => Some(progress::draw),
        _ => None,
    }
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
    answered(&mut drawn)?;

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
            shell.open_settings_page(Some(SettingsPage::Chat), cx);
        });
    });
    saved.push(drawn.save("settings-chat.png")?);
    saved.push(quitting(&mut drawn)?);
    let mut said: Vec<String> = (saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect();
    said.push("Stop Everything stopped the agent host, and nothing started it again".into());
    Ok(said)
}

/// A chat whose agent keeps a task running in the background, then Quit: the dialog that opens
/// is saved, and its Stop Everything stops the agent host, which stays stopped.
fn quitting(drawn: &mut Drawn) -> Result<std::path::PathBuf> {
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        shell.update(cx, Shell::close_settings);
    });
    let id = drawn.chat_saying(&checkout()?, IN_BACKGROUND)?;
    let chats = drawn.cx.read(|cx| shell.read(cx).chats().clone());
    drawn.until(PATIENCE, |drawn| {
        let background = |cx: &gpui_kit::App| {
            (chats.read(cx).summary(&id)).is_some_and(|summary| summary.beside.tasks > 0)
        };
        Ok(drawn.cx.read(background))
    })?;
    drawn.cx.update(|cx| cx.dispatch_action(&Quit));
    drawn.until(PATIENCE, |drawn| {
        Ok(drawn.cx.read(modal::current::<QuitDialog>).is_some())
    })?;
    let window = (drawn.cx.read(modal::window)).ok_or("the quit dialog has no window")?;
    (drawn.cx)
        .update_window(window, |_, window, cx| window.draw(cx).clear(cx))
        .map_err(|error| error.to_string())?;
    let image = (drawn.cx.capture_screenshot(window)).map_err(|error| error.to_string())?;
    let path = checkout()?.join(OUT).join("quit-dialog.png");
    image.save(&path).map_err(|error| error.to_string())?;
    let data = settings::data_dir().ok_or("there's no data folder")?;
    let socket = koyo_agent::client::host_dir(&data).socket();
    let dialog = (drawn.cx.read(modal::current::<QuitDialog>)).ok_or("the quit dialog closed")?;
    drawn.cx.update(|cx| dialog.update(cx, QuitDialog::end));
    drawn.until(PATIENCE, |_| Ok(!socket.exists()))?;
    let stopped = Instant::now();
    drawn.until(STAYS_STOPPED + PATIENCE, |_| {
        Ok(stopped.elapsed() >= STAYS_STOPPED)
    })?;
    if socket.exists() {
        return Err("the agent host started again after Stop Everything".into());
    }
    Ok(path)
}

fn draw_theme() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(THEME_OUT, &[])?;
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.dark_theme = blue_to_violet(DotKind::Free);
            settings.light_theme = blue_to_violet(DotKind::FixedLightness);
        });
    });
    drawn.chat_saying(&checkout()?, SAID_IN_MARKDOWN)?;
    answered(&mut drawn)?;
    let themes: [(&dyn Action, &str); 2] = [(&UseDarkTheme, "dark"), (&UseLightTheme, "light")];
    let mut said = Vec::new();
    for (theme, name) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        let path = drawn.save(&format!("chat-theme-{name}.png"))?;
        said.push(format!("saved {}", path.display()));
    }
    Ok(said)
}

/// Gives the chat the time it takes to take in what the fake provider did.
fn answered(drawn: &mut Drawn) -> Result<()> {
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| {
        Ok(start.elapsed() >= ANSWERED_IN)
    })
}

/// Two dots a quarter of the way out, blue (235°) and violet (275°): a deep blue going to violet
/// on the dark theme, as `Free` dots that near the centre are dark, and a pale one on the light,
/// as `FixedLightness` ones there are light.
fn blue_to_violet(kind: DotKind) -> GradientTheme {
    let at = |degrees: f32| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        point(180. + 48.5 * cos, 180. + 48.5 * sin)
    };
    GradientTheme {
        dots: vec![at(235.), at(275.)],
        kind,
        harmony: Harmony::Floating,
        lightness: 80.,
        ..GradientTheme::default()
    }
}
