//! The Chat settings page: what chats run on, what an agent may do and how hard it works in a new
//! chat, and how much of its work shows.

use std::path::Path;

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, SharedString, Window, actions};
use koyo_agent::protocol::model::Effort;
use koyo_agent::words::effort_label;

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::field_dialog::{self, Field};
use super::native_slider::NativeSlider;
use super::ticked_slider::nearest;
use crate::projects::display_path;
use crate::settings::{self, chat_mode_label, chat_mode_menu};

actions!(settings, [EditClaudePath, ClearClaudePath]);

/// The stops of the effort slider, one for each of [`Effort::ALL`].
const EFFORT_STOPS: [f32; 5] = [0., 1., 2., 3., 4.];

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![agent(cx), new_chats(cx), timeline(cx)]
}

fn effort_stop(effort: Effort) -> f32 {
    let mut stops = Effort::ALL.into_iter().zip(EFFORT_STOPS);
    stops.find(|(of, _)| *of == effort).map_or(0., |(_, stop)| stop)
}

fn effort_at(stop: f32) -> Effort {
    Effort::ALL.get(nearest(&EFFORT_STOPS, stop)).copied().unwrap_or(Effort::Xhigh)
}

/// Asks where the `claude` program is. Saving it empty leaves that to Koyo.
pub fn edit_claude_path(window: &mut Window, cx: &mut App) {
    let field = Field {
        ids: ["claude-path-cancel", "claude-path-save"],
        title: "Claude Code",
        note: "The program chats run on. Leave it empty for Koyo to look for it.".into(),
        placeholder: Some("e.g. /opt/homebrew/bin/claude"),
        value: settings::get(cx).claude_path.clone().unwrap_or_default(),
        save: |value, cx| {
            let path = (!value.is_empty()).then(|| value.to_owned());
            if path.as_deref().is_some_and(|path| !Path::new(path).is_file()) {
                return Err("There's no such file.".into());
            }
            settings::update(cx, |settings| settings.claude_path = path);
            Ok(())
        },
    };
    field_dialog::open(field, window, cx);
}

fn claude_label(path: Option<&str>) -> SharedString {
    path.map_or_else(|| "Found by Koyo".into(), |path| display_path(Path::new(path)).into())
}

fn claude_menu(path: Option<&str>) -> NativeMenu {
    NativeMenu::new()
        .menu_with_check("Found by Koyo", path.is_none(), Box::new(ClearClaudePath))
        .menu_with_check("Custom…", path.is_some(), Box::new(EditClaudePath))
}

fn agent(cx: &App) -> SettingsSection {
    let current = settings::get(cx);
    SettingsSection::new("Claude Code").row(
        SettingRow::new("Program")
            .description(
                "What chats run on. Koyo looks on your shell's PATH and where it's usually \
                 installed. Chats follow a change from the next time they start.",
            )
            .control(MenuButton::new(
                "claude-path",
                claude_label(current.claude_path.as_deref()),
                |cx| claude_menu(settings::get(cx).claude_path.as_deref()),
            )),
    )
}

fn new_chats(cx: &App) -> SettingsSection {
    let mode = settings::get(cx).chat_mode;
    SettingsSection::new("New chats")
        .row(
            SettingRow::new("Permissions")
                .description("What the agent may do without asking, until a chat says otherwise.")
                .control(MenuButton::new("chat-mode", chat_mode_label(mode), |cx| {
                    chat_mode_menu(settings::get(cx).chat_mode)
                })),
        )
        .row(
        SettingRow::new("Effort")
            .description(
                "How hard the agent works in a chat, until it's set to something else from the \
                 chat. A model that can't work that hard takes the nearest it can.",
            )
            .control(
                NativeSlider::new(
                    "chat-effort",
                    &EFFORT_STOPS,
                    |settings| effort_stop(settings.chat_effort),
                    |settings, stop| settings.chat_effort = effort_at(stop),
                )
                .ends(effort_label(Effort::Low), effort_label(Effort::Max)),
            ),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_effort_has_its_own_stop() {
        for effort in Effort::ALL {
            assert_eq!(effort_at(effort_stop(effort)), effort);
        }
    }
}
