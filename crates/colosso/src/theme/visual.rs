//! The General settings page, whose switches and slider are drawn in the accent, as the app
//! draws it in the light theme and the dark, saved as PNGs in `.superpowers/qa/accent/`, with
//! the accent this Mac's System Settings gave and where gpui-kit's theme took it:
//!
//! ```sh
//! COLOSSO_CONFIG_DIR=$PWD/.superpowers/qa/accent/config \
//!   cargo run -p colosso --features visual-check -- --visual-check-accent
//! ```

use std::process::ExitCode;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Action, App};

use super::accent::{self, hex};
use super::{UseDarkTheme, UseLightTheme, palette};
use crate::navigation::Route;
use crate::settings;
use crate::settings_view::SettingsPage;
use crate::visual::{Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/accent";

/// Draws the pictures and says where they are and what the accent was, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_in(OUT)?;
    let shell = drawn.shell.clone();
    let mut said = vec![drawn.cx.update(|cx| match accent::current(cx) {
        Some(read) => format!(
            "system accent read: {} light, {} dark",
            hex(read.light),
            hex(read.dark)
        ),
        None => "no system accent read; the app's own is in use".into(),
    })];
    drawn.cx.update(|cx| {
        // A switch that's on, whatever the check's own settings file says.
        settings::update(cx, |settings| settings.sidebar_open_on_launch = true);
        shell.update(cx, |shell, cx| {
            shell.navigate(Route::Settings(SettingsPage::General), cx);
        });
    });
    let themes: [(&dyn Action, &str); 2] = [(&UseLightTheme, "light"), (&UseDarkTheme, "dark")];
    for (theme, name) in themes {
        drawn.cx.update(|cx| cx.dispatch_action(theme));
        let path = drawn.save(&format!("settings-{name}.png"))?;
        said.push(drawn.cx.update(|cx| in_use(name, cx)));
        said.push(format!("saved {}", path.display()));
    }
    Ok(said)
}

/// The accent the palette and gpui-kit's theme hold now.
fn in_use(name: &str, cx: &App) -> String {
    let theme = cx.theme();
    format!(
        "{name}: palette accent {}, gpui-kit primary {}, slider bar {}, ring {}, selection {}, \
         accent {}",
        hex(palette(cx).accent),
        hex(theme.primary),
        hex(theme.slider_bar),
        hex(theme.ring),
        hex(theme.selection),
        hex(theme.accent),
    )
}
