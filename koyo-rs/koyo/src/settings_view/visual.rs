//! The General settings page drawn as the app draws it, its Keyboard section with the arrows'
//! repeat on and off, in the light theme and the dark, the Appearance page with its fonts as
//! they first show and with others picked, and a chat in the fake provider's words at the ends of
//! the text size's slider and in other fonts, saved as PNGs in `.superpowers/qa/settings/`:
//!
//! The agent host's folder is short, as a socket's path can't be long.
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
use koyo_agent::protocol::ids::MessageId;
use koyo_agent::protocol::methods::Delivery;
use koyo_agent::protocol::model::Prompt;

use super::SettingsPage;
use crate::files::Place;
use crate::key_repeat::KeyRepeat;
use crate::navigation::Route;
use crate::settings::{self, CHAT_SIZES, Fonts};
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result, checkout};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/settings";
/// How long the host gets to start a chat, or to say something in it.
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
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.fonts = Fonts {
                interface: Some("Georgia".to_owned()),
                code: Some("Menlo".to_owned()),
                chat_size: 22.,
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
    if std::env::var_os("KOYO_AGENT_PROVIDER").is_none_or(|provider| provider != "fake") {
        return Err("set KOYO_AGENT_PROVIDER=fake, so the chat doesn't go to Claude".into());
    }
    let root = checkout()?;
    let shell = drawn.shell.clone();
    let chats = drawn.cx.read(|cx| shell.read(cx).chats().clone());

    let made = drawn.cx.read(|cx| chats.read(cx).create(&root));
    let mut thread = None;
    drawn.finish(PATIENCE, async { thread = Some(made.await) })?;
    let id = thread
        .ok_or("the chat wasn't started")?
        .map_err(|error| format!("the chat wouldn't start: {error}"))?
        .id;

    let prompt = Prompt {
        message_id: MessageId::fresh(),
        text: SAID.to_owned(),
        attachments: Vec::new(),
    };
    let sent = drawn.cx.read(|cx| chats.read(cx).send(&id, prompt, Delivery::Auto));
    let mut said = None;
    drawn.finish(PATIENCE, async { said = Some(sent.await) })?;
    said.ok_or("nothing was said")?
        .map_err(|error| format!("the chat wouldn't take it: {error}"))?;

    drawn.cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Workspace, cx);
            shell.add_chat(&root, id.clone(), true, cx);
        });
        settings::update(cx, |settings| settings.fonts = Fonts::default());
    });
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| Ok(start.elapsed() >= ANSWERED_IN))?;

    let mut saved = Vec::new();
    let largest = CHAT_SIZES.last().copied().unwrap_or(22.);
    for size in [16., CHAT_SIZES[0], largest] {
        let fonts = Fonts { chat_size: size, ..Fonts::default() };
        drawn.cx.update(|cx| settings::update(cx, |settings| settings.fonts = fonts));
        saved.push(drawn.save(&format!("chat-{size}.png"))?);
    }
    let fonts = Fonts {
        interface: Some("Georgia".to_owned()),
        code: Some("Menlo".to_owned()),
        chat_size: 16.,
    };
    drawn.cx.update(|cx| settings::update(cx, |settings| settings.fonts = fonts));
    saved.push(drawn.save("chat-picked.png")?);
    Ok(saved)
}

/// A file in the editor in the bundled font, then another picked while it's open, which an
/// editor that kept its font from when it opened wouldn't take in.
fn file_in_its_fonts(drawn: &mut Drawn) -> Result<Vec<PathBuf>> {
    let (window, shell) = (drawn.window, drawn.shell.clone());
    drawn.cx.update(|cx| settings::update(cx, |settings| settings.fonts = Fonts::default()));
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.view_file(Place::file("koyo-rs/koyo/src/settings/fonts.rs"), window, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    let start = Instant::now();
    drawn.until(ANSWERED_IN + PATIENCE, |_| Ok(start.elapsed() >= ANSWERED_IN))?;
    drawn.pause()?;
    let mut saved = vec![drawn.save("file-bundled.png")?];
    drawn.cx.update(|cx| {
        settings::update(cx, |settings| settings.fonts.code = Some("Menlo".to_owned()));
    });
    saved.push(drawn.save("file-picked.png")?);
    Ok(saved)
}
