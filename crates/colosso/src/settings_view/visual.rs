//! The General settings page drawn as the app draws it, its Keyboard section with the arrows'
//! repeat on and off, in the light theme and the dark, saved as PNGs in
//! `.superpowers/qa/settings/`:
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/settings/config \
//!   cargo run -p colosso --features visual-check -- --visual-check-settings
//! ```

use std::process::ExitCode;

use gpui_kit::Action;

use super::SettingsPage;
use crate::key_repeat::KeyRepeat;
use crate::navigation::Route;
use crate::settings;
use crate::theme::{UseDarkTheme, UseLightTheme};
use crate::visual::{Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/settings";

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_in(OUT)?;
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| {
        // As the user first sees it, whatever the check's own settings file says.
        settings::update(cx, |settings| settings.key_repeat = KeyRepeat::default());
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
    Ok((saved.iter())
        .map(|path| format!("saved {}", path.display()))
        .collect())
}
