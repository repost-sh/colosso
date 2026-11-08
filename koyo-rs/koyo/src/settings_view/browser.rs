//! The Browser settings page: where a page's `DevTools` open, where a page goes full screen,
//! whether the new-tab launcher asks for search suggestions, the data every project's pages
//! keep, and the engine they run in.

use gpui_kit::{App, AsyncApp, PromptLevel};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::{browser, settings};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![devtools(cx), fullscreen(), address(), data(), engine(cx)]
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
                "Cookies, sign-ins, caches and site storage of every project's pages, each \
                 project's apart from the others', and the pages visited and searches made. \
                 Open pages load again.",
            )
            .control(
                PillButton::new("clear-browsing-data", "Clear…").on_click(|_, window, cx| {
                    let answer = window.prompt(
                        PromptLevel::Warning,
                        "Clear browsing data?",
                        Some("Every project's pages are signed out and forget what sites stored, and the pages visited are forgotten."),
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
