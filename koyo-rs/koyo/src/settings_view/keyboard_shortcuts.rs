//! Keyboard shortcuts: each command and the keys bound to it, read from the keymap.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::{Action, App, AsKeystroke as _, IntoElement, ParentElement as _, Styled as _, div};

use super::components::{SettingRow, SettingsSection};
use crate::arc::{NextProject, NextSpace, NextTab, PreviousProject, PreviousSpace, PreviousTab};
use crate::chat::{
    AllowRequest, AllowRequestForChat, DenyRequest, ImplementPlan, OpenModelMenu, OpenParent,
    RecallPrompt, SkipQuestion, StopTurn, ToggleDetails, TogglePlan,
};
use crate::files::Save;
use crate::go_to_file::GoToFile;
use crate::launcher::NewTab;
use crate::navigation::{CloseSettings, GoBack, GoForward, OpenSettings};
use crate::projects::NewWorktree;
use crate::shell::{
    CloseTab, ReopenClosedTab, SwitchTab, ToggleBottomPanel, ToggleRightPanel, ToggleSidebar,
    ToggleSidebarMode,
};
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
    (
        "Spaces",
        &[
            (
                "Show the project to the left, in this space or the one before",
                &PreviousProject,
            ),
            (
                "Show the project to the right, in this space or the next",
                &NextProject,
            ),
            ("Show the space to the left", &PreviousSpace),
            ("Show the space to the right", &NextSpace),
            ("Show the tab above in the sidebar", &PreviousTab),
            ("Show the tab below in the sidebar", &NextTab),
        ],
    ),
    ("Worktrees", &[("New worktree", &NewWorktree)]),
    (
        "Tabs",
        &[
            ("New tab: a browser tab, a chat or a terminal", &NewTab),
            ("Reopen the tab closed last", &ReopenClosedTab),
            ("Close the settings, or else the tab shown", &CloseTab),
            ("Switch between tabs, the latest shown first", &SwitchTab),
        ],
    ),
    (
        "Chat",
        &[
            ("Stop the agent", &StopTurn),
            ("Bring back the last prompt", &RecallPrompt),
            ("Choose the model", &OpenModelMenu),
            ("Plan first, or don't", &TogglePlan),
            ("Show or hide the chat's card", &ToggleDetails),
            ("Implement the plan", &ImplementPlan),
            ("Allow what's asked", &AllowRequest),
            ("Allow it for the rest of the chat", &AllowRequestForChat),
            ("Deny what's asked", &DenyRequest),
            ("Skip a question", &SkipQuestion),
            (
                "Back from a subagent to the chat it started in",
                &OpenParent,
            ),
        ],
    ),
    ("Files", &[("Go to file", &GoToFile), ("Save", &Save)]),
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

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::{SECTIONS, keys};
    use crate::chat;

    /// A row of the Chat or Spaces section that says "Not assigned" is a key someone unbound.
    #[gpui_kit::test]
    fn every_chat_and_space_command_has_its_key(cx: &mut TestAppContext) {
        cx.update(|cx| {
            chat::init(cx);
            crate::arc::bind_keys(cx);
            let sections = SECTIONS
                .iter()
                .filter(|(title, _)| ["Chat", "Spaces"].contains(title));
            for (title, commands) in sections {
                for (label, action) in *commands {
                    assert!(keys(*action, cx).is_some(), "{title}: {label} has no key");
                }
            }
        });
    }
}
