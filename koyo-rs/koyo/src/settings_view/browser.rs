//! The Browser settings page: the browsing profiles and the one each project's pages use,
//! where a page's `DevTools` open, where a page goes full screen, the search engine and whether
//! the new-tab launcher asks it for suggestions, whether a download asks where it goes, the data
//! every profile keeps, the engine pages run in, and Chrome's own settings.

use gpui_kit::{App, AsyncApp, ParentElement as _, PromptLevel, Styled as _, div, px};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::browser::profiles::{
    self, AgentPages, DeleteProfile, NewProfile, RenameProfile, SetAgentPages,
};
use crate::files::open_linked_page;
use crate::model::project_name;
use crate::projects::{display_path, pick_folder};
use crate::settings::checked_menu;
use crate::{browser, settings};

/// Chrome's settings page, the engine's own.
const CHROME_SETTINGS: &str = "chrome://settings/";

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![
        profiles(cx),
        project_profiles(cx),
        devtools(cx),
        fullscreen(),
        search(cx),
        downloads(cx),
        data(),
        engine(cx),
    ]
}

/// Every profile, with the projects on it, and making one (`browser/profiles.rs`); what
/// agents' pages are signed in as, and the Agents profile they use kept apart. Default and
/// Agents can't be renamed or deleted.
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
            .map(|project| project_name(project).to_string())
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
    let agents = kept.agents;
    let agent_pages = SettingRow::new("Agents' pages")
        .description(
            "The pages an agent opens in a project's worktrees. Signed in as you, they use the \
             project's profile, as your pages there do. Separate, they use the Agents profile, \
             with none of your sign-ins, and an agent asks you before using a page you opened.",
        )
        .control(MenuButton::new("agent-pages", agents.label(), |cx| {
            let agents = profiles::get(cx).agents;
            checked_menu(AgentPages::ALL, agents, AgentPages::label, |agents| {
                Box::new(SetAgentPages(agents))
            })
        }));
    let agents_profile = SettingRow::new(profiles::AGENTS_NAME).description(match agents {
        AgentPages::Shared => "Agents' pages, while they're kept separate. Unused now.",
        AgentPages::Separate => "Every project's agents' pages, apart from your sign-ins.",
    });
    (SettingsSection::new("Profiles").row(new).rows(listed))
        .row(agent_pages)
        .row(agents_profile)
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
            SettingRow::new(project_name(project))
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

fn search(cx: &App) -> SettingsSection {
    let engine = settings::get(cx).search_engine;
    let picked = SettingRow::new("Search engine")
        .description(
            "What you type in an address or the new-tab bar that isn't an address is searched \
             for here, as is text you search for from a page's menu.",
        )
        .control(MenuButton::new("search-engine", engine.label(), |cx| {
            settings::get(cx).search_engine.menu()
        }));
    let suggestions = SettingRow::new("Search suggestions")
        .description(format!(
            "While you type in the new-tab bar, ask {} what it suggests, without cookies. Off, \
             only your history and open tabs are offered.",
            engine.label()
        ))
        .control(NativeSwitch::new(
            "search-suggestions",
            |settings| settings.search_suggestions,
            |settings, on| settings.search_suggestions = on,
        ));
    SettingsSection::new("Search").row(picked).row(suggestions)
}

fn downloads(cx: &App) -> SettingsSection {
    let p = crate::theme::palette(cx);
    let current = settings::get(cx);
    let folder = (current.downloads_dir.clone()).or_else(dirs::download_dir).unwrap_or_default();
    let location = SettingRow::new("Location")
        .description("Downloads go here, with a number after a name a file here has already.")
        .control(div().max_w(px(220.)).truncate().text_sm().text_color(p.muted).child(display_path(&folder)))
        .control(PillButton::new("choose-downloads-dir", "Choose…").on_click(|_, _, cx| {
            let picked = pick_folder("Use Folder", cx);
            cx.spawn(async move |cx| {
                if let Some(folder) = picked.await {
                    cx.update(|cx| settings::update(cx, |settings| settings.downloads_dir = Some(folder)));
                }
            })
            .detach();
        }))
        .control(
            PillButton::new("reset-downloads-dir", "Reset")
                .disabled(current.downloads_dir.is_none())
                .on_click(|_, _, cx| settings::update(cx, |settings| settings.downloads_dir = None)),
        );
    let ask = SettingRow::new("Ask where to save each download")
        .description(
            "A page's download opens the save panel to pick where it goes. A page an agent \
             uses never asks.",
        )
        .control(NativeSwitch::new(
            "downloads-ask",
            |settings| settings.downloads_ask,
            |settings, on| settings.downloads_ask = on,
        ));
    SettingsSection::new("Downloads").row(location).row(ask)
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

/// The Chromium the pages run on, and Chrome's own settings for them, which open in a tab as
/// any of its pages does.
fn engine(cx: &App) -> SettingsSection {
    let version = match browser::chromium(cx) {
        Some(chromium) => format!("Chromium {chromium}"),
        None => "Packaged with the app; it starts with the first page.".to_owned(),
    };
    let chrome = SettingRow::new("Chrome settings")
        .description(
            "Chrome's own settings for pages: privacy and security, site permissions, autofill \
             and languages.",
        )
        .control(
            PillButton::new("chrome-settings", "Open").on_click(|_, window, cx| {
                open_linked_page(CHROME_SETTINGS, window, cx);
            }),
        );
    SettingsSection::new("Engine")
        .row(SettingRow::new("Browser engine").description(version))
        .row(chrome)
}
