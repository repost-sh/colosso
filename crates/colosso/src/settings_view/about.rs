//! About: which build is running and where its settings are kept.

use gpui_kit::{App, ParentElement as _, Styled as _, div};

use super::components::{PillButton, SettingRow, SettingsSection};
use crate::settings;
use crate::theme;

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let p = theme::palette(cx);
    let path = settings::path(cx).map(std::path::Path::to_path_buf);
    let location = path.as_ref().map_or_else(
        || "No folder to save settings in; changes last until Colosso quits.".to_string(),
        |path| path.display().to_string(),
    );
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
            .row(
                SettingRow::new("Settings file")
                    .description(location)
                    .control(
                        PillButton::new("reveal-settings-file", "Reveal in Finder")
                            .disabled(path.is_none())
                            .on_click(move |_, _, cx| {
                                if let Some(path) = &path {
                                    cx.reveal_path(path);
                                }
                            }),
                    ),
            ),
    ]
}
