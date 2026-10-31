//! Settings: a sidebar of pages that takes the workspace sidebar's place, and the page it
//! selects in the card. Each page lives in a module of its own.

mod about;
mod appearance;
mod browser;
mod components;
mod editor;
mod general;
mod keyboard_shortcuts;
mod languages;
mod native_slider;
mod native_switch;
#[cfg(all(target_os = "macos", feature = "visual-check"))]
pub mod native_visual;
mod settings_sidebar;
mod ticked_slider;
#[cfg(feature = "visual-check")]
pub mod visual;

use gpui_kit::assets::IconName;
use gpui_kit::{App, IntoElement, ParentElement as _};

pub use general::{EditBranchPrefix, edit_branch_prefix};
pub use settings_sidebar::SettingsSidebar;

use components::{SettingsPageColumn, SettingsSection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Editor,
    Languages,
    Appearance,
    Browser,
    KeyboardShortcuts,
    About,
}

/// The settings sidebar's headings and the pages under each, in order.
const SIDEBAR_GROUPS: [(&str, &[SettingsPage]); 2] = [
    (
        "Preferences",
        &[
            SettingsPage::General,
            SettingsPage::Editor,
            SettingsPage::Languages,
            SettingsPage::Appearance,
            SettingsPage::Browser,
            SettingsPage::KeyboardShortcuts,
        ],
    ),
    ("Colosso", &[SettingsPage::About]),
];

impl SettingsPage {
    pub fn title(self) -> &'static str {
        match self {
            SettingsPage::General => "General",
            SettingsPage::Editor => "Editor",
            SettingsPage::Languages => "Languages",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::Browser => "Browser",
            SettingsPage::KeyboardShortcuts => "Keyboard shortcuts",
            SettingsPage::About => "About",
        }
    }

    fn icon(self) -> IconName {
        match self {
            SettingsPage::General => IconName::Settings,
            SettingsPage::Editor => IconName::SquarePen,
            SettingsPage::Languages => IconName::Languages,
            SettingsPage::Appearance => IconName::SunMoon,
            SettingsPage::Browser => IconName::Globe,
            SettingsPage::KeyboardShortcuts => IconName::Keyboard,
            SettingsPage::About => IconName::Info,
        }
    }

    fn sections(self, cx: &App) -> Vec<SettingsSection> {
        match self {
            SettingsPage::General => general::sections(cx),
            SettingsPage::Editor => editor::sections(cx),
            SettingsPage::Languages => languages::sections(cx),
            SettingsPage::Appearance => appearance::sections(cx),
            SettingsPage::Browser => browser::sections(cx),
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
    use gpui_kit::{
        AppContext as _, Bounds, Context, Modifiers, Pixels, Render, Styled as _, TestAppContext,
        VisualTestContext, Window, div, point, px, size,
    };

    use super::components::SettingRow;
    use super::*;
    use crate::key_repeat::KeyRepeat;
    use crate::settings;
    use crate::test_support::in_window;

    /// The General page in a window of its own.
    struct General;

    impl Render for General {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(page(SettingsPage::General, cx))
        }
    }

    /// Clicks stop `index` of the `count` along the slider `selector` names.
    fn pick(selector: &'static str, index: usize, count: usize, cx: &mut VisualTestContext) {
        let track: Bounds<Pixels> = cx.debug_bounds(selector).unwrap();
        let along = track.size.width * (index as f32 / (count - 1) as f32);
        let at = point(track.left() + along, track.center().y);
        cx.simulate_click(at, Modifiers::none());
    }

    #[gpui_kit::test]
    #[expect(
        clippy::float_cmp,
        reason = "a stop is written as it's listed, and a value by hand left as it was"
    )]
    fn the_keyboard_section_sets_how_a_held_arrow_repeats(cx: &mut TestAppContext) {
        cx.update(crate::init);
        // Set by hand between two stops, it's shown at the nearer and left as it is.
        cx.update(|cx| {
            settings::update(cx, |settings| {
                settings.key_repeat = KeyRepeat {
                    per_second: 100.,
                    ..KeyRepeat::default()
                };
            });
        });
        // Tall enough for the whole page.
        let (window, _) = in_window(size(px(900.), px(1700.)), cx, |_, cx| cx.new(|_| General));
        let cx = &mut VisualTestContext::from_window(window, cx);
        cx.run_until_parked();
        let repeat = |cx: &mut VisualTestContext| cx.read(|cx| settings::get(cx).key_repeat);
        assert_eq!(repeat(cx).per_second, 100.);
        pick("arrow-repeat-speed", 7, 10, cx);
        assert_eq!(repeat(cx).per_second, 150.);
        // The longest delay is at the left, as macOS has it.
        pick("arrow-repeat-delay", 0, 8, cx);
        assert_eq!(repeat(cx).delay_ms, 500);
        // Off, the arrows are macOS's, and the two can't be moved.
        let switch = cx.debug_bounds("arrow-repeat").unwrap();
        cx.simulate_click(switch.center(), Modifiers::none());
        assert!(!repeat(cx).enabled);
        pick("arrow-repeat-speed", 2, 10, cx);
        assert_eq!(repeat(cx).per_second, 150.);
    }

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
            SettingsPage::Editor,
            SettingsPage::Languages,
            SettingsPage::Appearance,
            SettingsPage::KeyboardShortcuts,
            SettingsPage::About,
        ] {
            assert_eq!(listed.iter().filter(|&&&listed| listed == page).count(), 1);
        }
    }
}
