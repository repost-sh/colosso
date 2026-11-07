//! The Chat settings page: what chats run on and whether they have Koyo's tools, what an agent
//! may do and how hard it works in a new chat, how wide a chat is and what its composer shows
//! while it works, and how much of its work shows, its verbosity.

use std::path::Path;

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, SharedString, Window, actions};
use koyo_agent::protocol::model::Effort;
use koyo_agent::words::effort_label;

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::field_dialog::{self, Field};
use super::native_slider::NativeSlider;
use super::native_switch::NativeSwitch;
use super::ticked_slider::nearest;
use crate::projects::display_path;
use crate::settings::{self, Step, chat_mode_label, chat_mode_menu};

actions!(settings, [EditClaudePath, ClearClaudePath]);

/// The stops of the effort slider, one for each of [`Effort::ALL`].
const EFFORT_STOPS: [f32; 5] = [0., 1., 2., 3., 4.];

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![
        agent(cx),
        new_chats(cx),
        layout(cx),
        composer(cx),
        verbosity(cx),
    ]
}

fn effort_stop(effort: Effort) -> f32 {
    let mut stops = Effort::ALL.into_iter().zip(EFFORT_STOPS);
    stops
        .find(|(of, _)| *of == effort)
        .map_or(0., |(_, stop)| stop)
}

fn effort_at(stop: f32) -> Effort {
    Effort::ALL
        .get(nearest(&EFFORT_STOPS, stop))
        .copied()
        .unwrap_or(Effort::Xhigh)
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
            if path
                .as_deref()
                .is_some_and(|path| !Path::new(path).is_file())
            {
                return Err("There's no such file.".into());
            }
            settings::update(cx, |settings| settings.claude_path = path);
            Ok(())
        },
    };
    field_dialog::open(field, window, cx);
}

fn claude_label(path: Option<&str>) -> SharedString {
    path.map_or_else(
        || "Found by Koyo".into(),
        |path| display_path(Path::new(path)).into(),
    )
}

fn claude_menu(path: Option<&str>) -> NativeMenu {
    NativeMenu::new()
        .menu_with_check("Found by Koyo", path.is_none(), Box::new(ClearClaudePath))
        .menu_with_check("Custom…", path.is_some(), Box::new(EditClaudePath))
}

fn agent(cx: &App) -> SettingsSection {
    let current = settings::get(cx);
    SettingsSection::new("Claude Code")
        .row(
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
        .row(
            SettingRow::new("Koyo tools")
                .description(
                    "Chats work in Koyo with its own tools: they make worktrees, open pages in \
                     its browser and run programs in its terminals, where you see them. What a \
                     tool changes asks you first, as the chat's permissions say. Chats follow a \
                     change from the next time they start.",
                )
                .control(NativeSwitch::new(
                    "koyo-tools",
                    |settings| settings.koyo_tools,
                    |settings, on| settings.koyo_tools = on,
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

fn verbosity(cx: &App) -> SettingsSection {
    let verbosity = settings::get(cx).chat_verbosity;
    let preset = SettingRow::new("Verbosity")
        .description(
            "How much of the agent's work shows in a chat. Normal shows it as Claude Code does: \
             each change's diff, the end of what each command printed, the to-dos. What the agent \
             said always shows, and a click opens or closes any step.",
        )
        .control(MenuButton::new("chat-verbosity", verbosity.label(), |cx| {
            settings::get(cx).chat_verbosity.menu()
        }));
    let turns = SettingRow::new("Finished turns")
        .description("Whether a turn's work folds into one line once it's done.")
        .control(MenuButton::new(
            "chat-verbosity-turns",
            verbosity.turns_label(),
            |cx| settings::get(cx).chat_verbosity.turns_menu(),
        ));
    let steps = Step::ALL.into_iter().map(|step| {
        let id = SharedString::from(format!("chat-verbosity-{step:?}"));
        SettingRow::new(step.label())
            .description(step.description())
            .control(MenuButton::new(
                id,
                step.says(verbosity.shows(step)),
                move |cx| settings::get(cx).chat_verbosity.step_menu(step),
            ))
    });
    SettingsSection::new("Verbosity")
        .row(preset)
        .row(turns)
        .rows(steps)
}

fn layout(cx: &App) -> SettingsSection {
    let width = settings::get(cx).chat_width;
    SettingsSection::new("Layout").row(
        SettingRow::new("Width")
            .description(
                "How wide a chat's messages and work get: a column that reads easily, a wider \
                 one for code and diffs, or as wide as the pane. Right-click a chat to change \
                 this, its text size or its verbosity there.",
            )
            .control(MenuButton::new("chat-width", width.label(), |cx| {
                settings::get(cx).chat_width.menu()
            })),
    )
}

fn composer(cx: &App) -> SettingsSection {
    let activity = settings::get(cx).chat_activity;
    SettingsSection::new("Composer").row(
        SettingRow::new("While it works")
            .description(
                "How the composer's edge shows the agent or one of its subagents working: a subtle \
                 line going slowly round it, a clearer one, or the edge a little brighter, holding \
                 still, as a line does with Reduce motion on or Koyo in the background. Off leaves \
                 it as faint as any glass's.",
            )
            .control(MenuButton::new("chat-activity", activity.label(), |cx| {
                settings::get(cx).chat_activity.menu()
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
