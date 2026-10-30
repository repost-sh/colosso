//! Keyboard shortcuts: each command and the keys bound to it, read from the keymap.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::{Action, App, AsKeystroke as _, IntoElement, ParentElement as _, Styled as _, div};

use super::components::{SettingRow, SettingsSection};
use crate::files::{CloseTab, Save, SwitchTab};
use crate::go_to_file::GoToFile;
use crate::navigation::{CloseSettings, GoBack, GoForward, OpenSettings};
use crate::projects::NewWorktree;
use crate::shell::{NewChat, ToggleBottomPanel, ToggleRightPanel, ToggleSidebar, ToggleSidebarMode};
use crate::theme::{self, CycleTheme};

/// A command as the page names it, and the action it runs.
type Command = (&'static str, &'static dyn Action);

/// Each section's commands, in the order the page lists them.
const SECTIONS: &[(&str, &[Command])] = &[
    (
        "Navigation",
        &[
            ("Open settings", &OpenSettings),
            ("Close settings", &CloseSettings),
            ("Back", &GoBack),
            ("Forward", &GoForward),
        ],
    ),
    (
        "Window",
        &[
            ("Toggle sidebar", &ToggleSidebar),
            ("Switch between Orca and Arc layouts", &ToggleSidebarMode),
            ("Toggle right panel", &ToggleRightPanel),
            ("Toggle bottom panel", &ToggleBottomPanel),
            ("Cycle theme", &CycleTheme),
        ],
    ),
    ("Worktrees", &[("New worktree", &NewWorktree)]),
    (
        "Tabs",
        &[
            ("New chat", &NewChat),
            ("Close the settings, the file shown, or else the tab in the middle", &CloseTab),
        ],
    ),
    (
        "Files",
        &[
            ("Go to file", &GoToFile),
            ("Switch between the open files", &SwitchTab),
            ("Save", &Save),
        ],
    ),
];

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let muted = theme::palette(cx).muted;
    SECTIONS
        .iter()
        .map(|(title, commands)| {
            SettingsSection::new(*title).rows(commands.iter().map(|(label, action)| {
                SettingRow::new(*label).control(match keys(*action, cx) {
                    Some(keys) => keys.into_any_element(),
                    None => div()
                        .text_sm()
                        .text_color(muted)
                        .child("Not assigned")
                        .into_any_element(),
                })
            }))
        })
        .collect()
}

/// The first keystroke of the binding that wins for `action`, whatever key context the binding
/// is limited to: this page lists every shortcut, not only those live where focus is.
fn keys(action: &dyn Action, cx: &App) -> Option<Kbd> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    // The keymap lists later bindings last, and later bindings take precedence.
    let binding = keymap.bindings_for_action(action).last()?;
    let keystroke = binding.keystrokes().first()?.as_keystroke().clone();
    Some(Kbd::new(keystroke))
}
