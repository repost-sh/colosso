//! Keyboard shortcuts: each command and the keys bound to it, read from the live keymap.

use gpui_kit::component::kbd::Kbd;
use gpui_kit::{Action, App, IntoElement, ParentElement as _, Styled as _, Window, div};

use super::components::{SettingRow, SettingsSection};
use crate::navigation::{CloseSettings, GoBack, GoForward, OpenSettings};
use crate::shell::{
    NewChat, ToggleBottomPanel, ToggleRightPanel, ToggleSidebar, ToggleSidebarMode,
};
use crate::theme::{self, CycleTheme};

struct Shortcut {
    label: &'static str,
    action: fn() -> Box<dyn Action>,
}

const SECTIONS: [(&str, &[Shortcut]); 3] = [
    (
        "Navigation",
        &[
            Shortcut {
                label: "Open settings",
                action: || Box::new(OpenSettings),
            },
            Shortcut {
                label: "Close settings",
                action: || Box::new(CloseSettings),
            },
            Shortcut {
                label: "Back",
                action: || Box::new(GoBack),
            },
            Shortcut {
                label: "Forward",
                action: || Box::new(GoForward),
            },
        ],
    ),
    (
        "Window",
        &[
            Shortcut {
                label: "Toggle sidebar",
                action: || Box::new(ToggleSidebar),
            },
            Shortcut {
                label: "Switch between Orca and Arc layouts",
                action: || Box::new(ToggleSidebarMode),
            },
            Shortcut {
                label: "Toggle right panel",
                action: || Box::new(ToggleRightPanel),
            },
            Shortcut {
                label: "Toggle bottom panel",
                action: || Box::new(ToggleBottomPanel),
            },
            Shortcut {
                label: "Cycle theme",
                action: || Box::new(CycleTheme),
            },
        ],
    ),
    (
        "Tabs",
        &[Shortcut {
            label: "New chat",
            action: || Box::new(NewChat),
        }],
    ),
];

pub fn search_terms() -> Vec<&'static str> {
    SECTIONS
        .iter()
        .flat_map(|(title, shortcuts)| {
            std::iter::once(*title).chain(shortcuts.iter().map(|shortcut| shortcut.label))
        })
        .collect()
}

pub fn sections(window: &mut Window, cx: &App) -> Vec<impl IntoElement> {
    let muted = theme::palette(cx).muted;
    SECTIONS
        .iter()
        .map(|(title, shortcuts)| {
            SettingsSection::new(*title).children(shortcuts.iter().map(|shortcut| {
                let keys = Kbd::binding_for_action(&*(shortcut.action)(), None, window);
                SettingRow::new(shortcut.label).control(match keys {
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
