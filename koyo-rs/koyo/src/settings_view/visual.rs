//! The General settings page drawn as the app draws it, its Keyboard section with the arrows'
//! repeat on and off, in the light theme and the dark, the Appearance page with its fonts as
//! they first show and with others picked, the Agent desktop page with one grant held and one
//! not, and a chat in the fake provider's words at the ends of
//! the text size's slider and in other fonts, saved as PNGs in `.superpowers/qa/settings/`:
//!
//! ```sh
//! mkdir -p .superpowers/c/tmp &&
//!   KOYO_AGENT_PROVIDER=fake KOYO_DATA_DIR=$PWD/.superpowers/c TMPDIR=$PWD/.superpowers/c/tmp \
//!   KOYO_CONFIG_DIR=$PWD/.superpowers/qa/settings/config KOYO_LOG_DIR=$PWD/.superpowers/c/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-settings
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use gpui_kit::Action;

use super::SettingsPage;
use crate::files::Place;
use crate::key_repeat::KeyRepeat;
use crate::navigation::Route;
use crate::settings::{self, CHAT_SIZES, Fonts, LARGEST_CHAT_SIZE};
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/settings";
/// How much longer than it waits for a wait may go on, with the app slow to draw.
const PATIENCE: Duration = Duration::from_secs(20);
/// How long the chat gets to take in what the fake provider says back.
const ANSWERED_IN: Duration = Duration::from_secs(4);
/// What's told to the chat, whose answer says it back as Markdown.
const SAID: &str = "a paragraph with **bold** and `inline code` in it.

## A heading

```rust
fn main() {
    let arrow = \"a -> b != c\";
}
```

- One
- Two
";

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        // As the user first sees it, whatever the check's own settings file says.
        settings::update(cx, |settings| {
            settings.key_repeat = KeyRepeat::default();
            settings.fonts = Fonts::default();
        });
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::General), cx);
        });
    });
    let mut saved = Vec::new();
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        drawn.cx.update(|cx| {
            cx.dispatch_action(theme);
            settings::update(cx, |settings| settings.key_repeat.enabled = true);
        });
        saved.push(drawn.save(&format!("general-{name}.png"))?);
        drawn.cx.update(|cx| {
            settings::update(cx, |settings| settings.key_repeat.enabled = false);
        });
        saved.push(drawn.save(&format!("general-repeat-off-{name}.png"))?);
    }
    drawn.cx.update(|cx| {
        cx.dispatch_action(&UseLightTheme);
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::Appearance), cx);
        });
    });
    saved.push(drawn.save("appearance.png")?);
    saved.push(agent_desktop(&mut drawn)?);
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.fonts = Fonts {
                interface: Some("Georgia".to_owned()),
                code: Some("Menlo".to_owned()),
                chat_size: LARGEST_CHAT_SIZE,
            };
        });
    });
    saved.push(drawn.save("appearance-picked.png")?);
    saved.extend(chat_in_its_sizes(&mut drawn)?);
    saved.extend(file_in_its_fonts(&mut drawn)?);
    Ok((saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect())
}

/// A chat with something said in it, at each end of the slider, at the default, and in other
/// fonts.
fn chat_in_its_sizes(drawn: &mut Drawn) -> Result<Vec<PathBuf>> {
    drawn.chat_saying(&checkout()?, SAID)?;
    drawn
        .cx
        .update(|cx| settings::update(cx, |settings| settings.fonts = Fonts::default()));
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| {
        Ok(start.elapsed() >= ANSWERED_IN)
    })?;

    let mut saved = Vec::new();
    let default = Fonts::default().chat_size;
    for size in [default, CHAT_SIZES[0], LARGEST_CHAT_SIZE] {
        let fonts = Fonts {
            chat_size: size,
            ..Fonts::default()
        };
        drawn
            .cx
            .update(|cx| settings::update(cx, |settings| settings.fonts = fonts));
        saved.push(drawn.save(&format!("chat-{size}.png"))?);
    }
    let fonts = Fonts {
        interface: Some("Georgia".to_owned()),
        code: Some("Menlo".to_owned()),
        chat_size: default,
    };
    drawn
        .cx
        .update(|cx| settings::update(cx, |settings| settings.fonts = fonts));
    saved.push(drawn.save("chat-picked.png")?);
    Ok(saved)
}

/// A file in the editor in the bundled font, then another picked while it's open, which an
/// editor that kept its font from when it opened wouldn't take in.
fn file_in_its_fonts(drawn: &mut Drawn) -> Result<Vec<PathBuf>> {
    let (window, shell) = (drawn.window, drawn.shell.clone());
    drawn
        .cx
        .update(|cx| settings::update(cx, |settings| settings.fonts = Fonts::default()));
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.view_file(
                    Place::file("koyo-rs/koyo/src/settings/fonts.rs"),
                    window,
                    cx,
                );
            });
        })
        .map_err(|error| error.to_string())?;
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| {
        Ok(start.elapsed() >= ANSWERED_IN)
    })?;
    drawn.pause()?;
    let mut saved = vec![drawn.save("file-bundled.png")?];
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.fonts.code = Some("Menlo".to_owned());
        });
    });
    saved.push(drawn.save("file-picked.png")?);
    Ok(saved)
}

/// The agent desktop's page with one grant held and one not, each marked.
fn agent_desktop(drawn: &mut Drawn) -> Result<PathBuf> {
    let (shell, desk) = (drawn.shell.clone(), drawn.cx.update(|cx| crate::agent_desktop::desk(cx)));
    let stands = |drawn: &mut Drawn, state, grants| {
        drawn.cx.update(|cx| {
            desk.update(cx, |desk, cx| {
                (desk.state, desk.grants) = (state, grants);
                cx.notify();
            });
        });
    };
    let grants = crate::agent_desktop::Grants {
        accessibility: true,
        screen_recording: false,
    };
    stands(drawn, crate::agent_desktop::State::Running, Some(grants));
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.navigate(Route::Settings(SettingsPage::AgentDesktop), cx)));
    let saved = drawn.save("agent-desktop.png");
    stands(drawn, crate::agent_desktop::State::Off, None);
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.navigate(Route::Settings(SettingsPage::Appearance), cx)));
    saved
}
