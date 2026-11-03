//! The Browser settings page: where a page's `DevTools` open, the data every project's pages
//! keep, and the engine they run in.

use gpui_kit::{App, AsyncApp, PromptLevel};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use crate::{browser, settings};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![devtools(cx), data(), engine(cx)]
}

fn devtools(cx: &App) -> SettingsSection {
    let dock = settings::get(cx).devtools.dock;
    SettingsSection::new("DevTools").row(
        SettingRow::new("DevTools open")
            .description(
                "Docked at a side of the page, past a divider you drag, or in a pane of their \
                 own beside it. Moving them in their bar changes where the next ones open.",
            )
            .control(MenuButton::new("devtools-dock", dock.label(), |cx| {
                settings::get(cx).devtools.dock.menu()
            })),
    )
}

fn data() -> SettingsSection {
    SettingsSection::new("Browsing data").row(
        SettingRow::new("Clear browsing data")
            .description(
                "Cookies, sign-ins, caches and site storage of every project's pages. Each \
                 project keeps its own, apart from the others'. Open pages load again.",
            )
            .control(
                PillButton::new("clear-browsing-data", "Clear…").on_click(|_, window, cx| {
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "Clear browsing data?",
                        Some("Every project's pages are signed out and forget what sites stored."),
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
