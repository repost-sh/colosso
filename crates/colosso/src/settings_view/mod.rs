//! Settings: a sidebar of pages that takes the workspace sidebar's place, and the page it
//! selects in the card. Each page lives in a module of its own.

mod about;
mod appearance;
mod components;
mod general;
mod keyboard_shortcuts;
mod settings_sidebar;

use gpui_kit::assets::IconName;
use gpui_kit::{App, IntoElement, ParentElement as _};

pub use settings_sidebar::SettingsSidebar;

use components::{SettingsPageColumn, SettingsSection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    fn sections(self, cx: &App) -> Vec<SettingsSection> {
        match self {
            SettingsPage::General => general::sections(cx),
            SettingsPage::Appearance => appearance::sections(cx),
            SettingsPage::KeyboardShortcuts => keyboard_shortcuts::sections(cx),
            SettingsPage::About => about::sections(cx),
        }
    }

    /// Whether the page's title or anything on it mentions `query`, which must already be
    /// trimmed and lowercase. Searches the same sections the page shows.
    fn matches(self, query: &str, cx: &App) -> bool {
        query.is_empty()
            || self.title().to_lowercase().contains(query)
            || self
                .sections(cx)
                .iter()
                .any(|section| section.matches(query))
    }
}

/// The page's contents for the card.
pub fn page(page: SettingsPage, cx: &App) -> impl IntoElement {
    SettingsPageColumn::new(page).children(page.sections(cx))
}

#[cfg(test)]
mod tests {
    use super::components::SettingRow;
    use super::*;

    #[test]
    fn a_section_matches_its_title_and_its_rows() {
        let section = SettingsSection::new("Sidebar")
            .row(SettingRow::new("Sidebar width").description("Drag the sidebar's right edge."));
        assert!(section.matches("sidebar"));
        assert!(section.matches("width"));
        assert!(section.matches("right edge"));
        assert!(!section.matches("theme"));
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
