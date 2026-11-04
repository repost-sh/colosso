//! The Chat settings page: how much of an agent's work shows in a chat.

use gpui_kit::App;

use super::components::{MenuButton, SettingRow, SettingsSection};
use crate::settings;

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![timeline(cx)]
}

fn timeline(cx: &App) -> SettingsSection {
    let work = settings::get(cx).chat_work;
    SettingsSection::new("Timeline").row(
        SettingRow::new("Work to show")
            .description(
                "How much of the tools an agent ran and what it thought shows in a chat. What it \
                 said always shows.",
            )
            .control(MenuButton::new("chat-work", work.label(), |cx| {
                settings::get(cx).chat_work.menu()
            })),
    )
}
