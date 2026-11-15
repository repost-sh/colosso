//! The Browser settings page: the browsing profiles and the one each project's pages use,
//! where a page's `DevTools` open, where a page goes full screen, whether the new-tab launcher
//! asks for search suggestions, the data every profile keeps, and the engine pages run in.

use gpui_kit::{App, AsyncApp, ParentElement as _, PromptLevel, Styled as _, div, px};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::browser::profiles::{self, DeleteProfile, NewProfile, RenameProfile};
use crate::model::project_name;
use crate::{browser, settings};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![
        profiles(cx),
        project_profiles(cx),
        devtools(cx),
        fullscreen(),
        address(),
        data(),
        engine(cx),
    ]
}

/// Every profile, with the projects on it, and making one (`browser/profiles.rs`). Default
/// can't be renamed or deleted.
fn profiles(cx: &App) -> SettingsSection {
    let (settings, kept) = (settings::get(cx), profiles::get(cx));
    let new = SettingRow::new("New profile")
        .description(
            "A profile keeps its own cookies, sign-ins and site data. A project's pages, in all \
             its worktrees, use the profile it's on, and projects on the same profile are signed \
             in alike. Pages opened outside any project use Default.",
        )
        .control(
            PillButton::new("profile-new", "New Profile…").on_click(|_, window, cx| {
                window.dispatch_action(Box::new(NewProfile(None)), cx);
            }),
        );
    let listed = kept.all().enumerate().map(|(index, (id, name))| {
        let names: Vec<String> = (kept.on(id, &settings.projects))
            .map(|project| project_name(&settings.project_looks, project).to_string())
            .collect();
        let used = match names.as_slice() {
            [] => "No project uses it.".to_owned(),
            [one] => format!("Used by {one}."),
            [rest @ .., last] => format!("Used by {} and {last}.", rest.join(", ")),
        };
        let row = SettingRow::new(name.to_owned());
        if id == profiles::DEFAULT {
            return row.description(format!(
                "Pages opened outside any project, and new projects. {used}"
            ));
        }
        let (rename, delete) = (RenameProfile(id.to_owned()), DeleteProfile(id.to_owned()));
        let buttons = div()
            .flex()
            .gap(px(6.))
            .child(
                PillButton::new(("profile-rename", index), "Rename…").on_click(
                    move |_, window, cx| window.dispatch_action(Box::new(rename.clone()), cx),
                ),
            )
            .child(
                PillButton::new(("profile-delete", index), "Delete…").on_click(
                    move |_, window, cx| window.dispatch_action(Box::new(delete.clone()), cx),
                ),
            );
        row.description(used).control(buttons)
    });
    SettingsSection::new("Profiles").row(new).rows(listed)
}

/// The profile each project's pages use, picked as from its Set Profile menu.
fn project_profiles(cx: &App) -> SettingsSection {
    let (settings, kept) = (settings::get(cx), profiles::get(cx));
    let rows = settings
        .projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            let on = kept.name(kept.of(project)).to_owned();
            let path = project.clone();
            SettingRow::new(project_name(&settings.project_looks, project))
                .description(project.display().to_string())
                .control(MenuButton::new(("project-profile", index), on, move |cx| {
                    profiles::menu(&path, cx)
                }))
        });
    SettingsSection::new("Project profiles").rows(rows)
}

fn devtools(cx: &App) -> SettingsSection {
    let dock = settings::get(cx).devtools.dock;
    SettingsSection::new("DevTools").row(
        SettingRow::new("DevTools open")
            .description(
                "Docked at a side of the page, past a divider you drag, or in a pane of their \
                 own beside it. Moving them from Dock side in their ⋮ menu changes where the \
                 next ones open.",
            )
            .control(MenuButton::new("devtools-dock", dock.label(), |cx| {
                settings::get(cx).devtools.dock.menu()
            })),
    )
}

fn fullscreen() -> SettingsSection {
    SettingsSection::new("Full screen").row(
        SettingRow::new("Full screen fills the screen")
            .description(
                "A page that goes full screen, as a video does, takes the whole screen, the \
                 window full screen with that page alone. Off, it fills its pane and the window \
                 stays as it is. Escape leaves full screen.",
            )
            .control(NativeSwitch::new(
                "fullscreen-fills-screen",
                |settings| settings.fullscreen_fills_screen,
                |settings, on| settings.fullscreen_fills_screen = on,
            )),
    )
}

fn address() -> SettingsSection {
    SettingsSection::new("New tab").row(
        SettingRow::new("Search suggestions")
            .description(
                "While you type in the new-tab bar, ask Google what it suggests, without cookies. \
                 Off, only your history and open tabs are offered.",
            )
            .control(NativeSwitch::new(
                "search-suggestions",
                |settings| settings.search_suggestions,
                |settings, on| settings.search_suggestions = on,
            )),
    )
}

fn data() -> SettingsSection {
    SettingsSection::new("Browsing data").row(
        SettingRow::new("Clear browsing data")
            .description(
                "Cookies, sign-ins, caches and site storage of every profile's pages, each \
                 profile's apart from the others', and the pages visited and searches made. \
                 Open pages load again.",
            )
            .control(
                PillButton::new("clear-browsing-data", "Clear…").on_click(|_, window, cx| {
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "Clear browsing data?",
                        Some("Every profile's pages are signed out and forget what sites stored, and the pages visited are forgotten."),
                        &["Clear", "Cancel"],
                        cx,
                    );
                    cx.spawn(async move |cx: &mut AsyncApp| {
                        if answer.await == Ok(0) {
                            cx.update(browser::clear_data);
                        }
                    })
                    .detach();
                }),
            ),
    )
}

fn engine(cx: &App) -> SettingsSection {
    let description = match browser::versions(cx) {
        Some((engine, chromium)) => format!("Chromium {chromium}, in engine {engine}."),
        None => "Packaged with the app; it starts with the first page.".to_owned(),
    };
    SettingsSection::new("Engine").row(SettingRow::new("Browser engine").description(description))
}
