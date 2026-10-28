//! Settings: a sidebar of pages that takes the workspace sidebar's place, and the page it
//! selects in the card. Each page lives in a module of its own.

mod about;
mod appearance;
mod components;
mod general;
mod keyboard_shortcuts;
mod settings_sidebar;

use gpui_kit::assets::IconName;
use gpui_kit::{App, IntoElement, ParentElement as _, Window};

pub use settings_sidebar::SettingsSidebar;

use components::SettingsPageColumn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsPage {
    General,
    Appearance,
    KeyboardShortcuts,
    About,
}

/// The settings sidebar's headings and the pages under each, in order.
const SIDEBAR_GROUPS: [(&str, &[SettingsPage]); 2] = [
    (
        "Preferences",
        &[
            SettingsPage::General,
            SettingsPage::Appearance,
            SettingsPage::KeyboardShortcuts,
        ],
    ),
    ("Colosso", &[SettingsPage::About]),
];

impl SettingsPage {
    pub fn title(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::KeyboardShortcuts => "Keyboard shortcuts",
            SettingsPage::About => "About",
        }
    }

    fn icon(self) -> IconName {
        match self {
            SettingsPage::General => IconName::Settings,
            SettingsPage::Appearance => IconName::SunMoon,
            SettingsPage::KeyboardShortcuts => IconName::Keyboard,
            SettingsPage::About => IconName::Info,
        }
    }

    /// Whether the page's title or any setting on it mentions `query`, ignoring case.
    fn matches(self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        let terms = match self {
            SettingsPage::General => general::SEARCH_TERMS.to_vec(),
            SettingsPage::Appearance => appearance::SEARCH_TERMS.to_vec(),
            SettingsPage::KeyboardShortcuts => keyboard_shortcuts::search_terms(),
            SettingsPage::About => about::SEARCH_TERMS.to_vec(),
        };
        std::iter::once(self.title())
            .chain(terms)
            .any(|term| term.to_lowercase().contains(&query))
    }
}

/// The page's contents for the card.
pub fn page(page: SettingsPage, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let column = SettingsPageColumn::new(page);
    match page {
        SettingsPage::General => column.children(general::sections(cx)),
        SettingsPage::Appearance => column.children(appearance::sections(cx)),
        SettingsPage::KeyboardShortcuts => {
            column.children(keyboard_shortcuts::sections(window, cx))
        }
        SettingsPage::About => column.children(about::sections(cx)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_pages_by_title_and_by_setting() {
        assert!(SettingsPage::Appearance.matches("appear"));
        assert!(SettingsPage::General.matches("  SIDEBAR width "));
        assert!(SettingsPage::KeyboardShortcuts.matches("bottom panel"));
        assert!(!SettingsPage::About.matches("sidebar"));
    }

    #[test]
    fn every_page_is_in_the_sidebar_once() {
        let listed: Vec<_> = SIDEBAR_GROUPS
            .iter()
            .flat_map(|(_, pages)| pages.iter())
            .collect();
        for page in [
            SettingsPage::General,
            SettingsPage::Appearance,
            SettingsPage::KeyboardShortcuts,
            SettingsPage::About,
        ] {
            assert_eq!(listed.iter().filter(|&&&listed| listed == page).count(), 1);
        }
    }
}
