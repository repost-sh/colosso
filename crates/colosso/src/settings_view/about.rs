//! About: which build is running, where its settings are kept and where it logs.

use std::path::Path;

use gpui_kit::{App, ParentElement as _, Styled as _, div};

use super::components::{PillButton, SettingRow, SettingsSection};
use crate::projects::display_path;
use crate::{logs, settings, theme};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let p = theme::palette(cx);
    vec![
        SettingsSection::new("Colosso")
            .row(
                SettingRow::new("Version").control(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(env!("CARGO_PKG_VERSION")),
                ),
            )
            .row(file_row(
                "Settings file",
                "reveal-settings-file",
                settings::path(cx),
                "No folder to save settings in; changes last until Colosso quits.",
            ))
            .row(file_row(
                "Log file",
                "reveal-log-file",
                logs::file(),
                "Logging to the terminal Colosso was started from.",
            )),
    ]
}

/// A row saying where a file is kept, with a button to show it, or why there isn't one.
fn file_row(
    title: &'static str,
    id: &'static str,
    path: Option<&Path>,
    missing: &'static str,
) -> SettingRow {
    let path = path.map(Path::to_path_buf);
    SettingRow::new(title)
        .description(
            path.as_deref()
                .map_or_else(|| missing.to_owned(), display_path),
        )
        .control(
            PillButton::new(id, "Reveal in Finder")
                .disabled(path.is_none())
                .on_click(move |_, _, cx| {
                    if let Some(path) = &path {
                        cx.reveal_path(path);
                    }
                }),
        )
}
