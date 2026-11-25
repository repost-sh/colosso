//! Appearance: light, dark or the system's, and the gradient glass of each; the layout: where the
//! tabs, the address bar and the Changes and Files buttons go, whether the unpinned pages are each
//! space's or shared, and how much of the window's glass the sidebar lets through
//! (`docs/layout.md`, Settings); the wallpaper behind chats; and the fonts.

use gpui_kit::App;
use gpui_kit::component::ActiveTheme as _;

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection, slider};
use super::native_slider::NativeSlider;
use super::{chat_backdrop, fonts};
use crate::gradient::{self, GradientTheme};
use crate::picker;
use crate::settings::{self, GLASS_STOPS, Settings};
use crate::theme;

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let preference = theme::preference(cx);
    let appearance = if cx.theme().mode.is_dark() {
        "dark"
    } else {
        "light"
    };
    let glass_is_default = *gradient::current(cx) == GradientTheme::default();
    vec![
        SettingsSection::new("Theme")
            .row(
                SettingRow::new("Appearance")
                    .description("Follow the system, or keep Koyo light or dark.")
                    .control(MenuButton::new("appearance", preference.label(), |cx| {
                        theme::preference(cx).menu()
                    })),
            )
            .row(
                SettingRow::new("Glass")
                    .description(format!(
                        "The gradient tinting the window's glass in the {appearance} appearance. \
                     Light and dark each keep their own."
                    ))
                    .control(
                        PillButton::new("reset-glass", "Reset")
                            .disabled(glass_is_default)
                            .on_click(|_, _, cx| {
                                gradient::update(cx, |glass| *glass = GradientTheme::default());
                            }),
                    )
                    .control(PillButton::new("edit-glass", "Edit…").on_click(
                        |event, window, cx| {
                            picker::open(event.position(), window, cx);
                        },
                    )),
            ),
        layout(cx),
        chat_backdrop::section(cx),
        fonts::section(cx),
    ]
}

/// The layout, in `docs/layout.md`'s order: where the tabs, the address bar and the Changes and
/// Files buttons go, the unpinned pages, the sidebar's glass.
fn layout(cx: &App) -> SettingsSection {
    let current = settings::get(cx);
    let loose = current.loose_pages;
    SettingsSection::new("Layout")
        .rows(placement_rows(current))
        .row(
            SettingRow::new("Unpinned pages")
                .description(
                    "The pages after the pinned ones that are no branch's. Per space, each space \
                     lists its own; Shared, every space lists them all. A \
                     page opened while they're shared is the space's shown then, so going back \
                     to Per space puts each in its space. Pinned pages are in every space.",
                )
                .control(MenuButton::new("loose-pages", loose.label(), |cx| {
                    settings::get(cx).loose_pages.menu()
                })),
        )
        .row(sidebar_glass_row())
}

/// The rows for where the tabs, the address bar and the Changes and Files buttons go, each
/// showing where it is: Changes and Files on the branch's row while the tabs are in the sidebar,
/// its menu's Top rail greyed then.
fn placement_rows(current: &Settings) -> [SettingRow; 3] {
    [
        SettingRow::new("Tabs")
            .description(
                "Where every tab goes, together: the pinned and unpinned pages, the Settings tab \
                 and the selected branch's tabs. Across the top rail, or down the sidebar.",
            )
            .control(MenuButton::new("tabs", current.tabs.label(), |cx| {
                settings::get(cx).tabs.menu()
            })),
        SettingRow::new("Address bar")
            .description(
                "Where the page's address goes, with back, forward and reload: in a field at the \
                 top of the sidebar, or in each page's own toolbar.",
            )
            .control(MenuButton::new(
                "address-bar",
                current.address_bar.label(),
                |cx| settings::get(cx).address_bar.menu(),
            )),
        SettingRow::new("Changes and Files")
            .description(
                "Where the selected branch's Changes and Files buttons go: on its row in the \
                 sidebar, or at the top rail's right end. Top rail is offered only while the \
                 tabs are on it.",
            )
            .control(MenuButton::new(
                "changes-and-files",
                current.changes_and_files_shown().label(),
                |cx| settings::get(cx).changes_and_files_menu(),
            )),
    ]
}

/// The row for how much of the window's glass shows through the sidebar.
fn sidebar_glass_row() -> SettingRow {
    let glass = NativeSlider::new(
        "sidebar-glass",
        &GLASS_STOPS,
        |settings| settings.sidebar_glass,
        |settings, stop| settings.sidebar_glass = stop,
    );
    slider(
        (
            "Sidebar glass",
            "How much of the window's glass shows through the sidebar: the left end is clear, \
             the middle the card's own, the right end solid.",
        ),
        glass,
        ("Clear", "Solid"),
    )
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::{layout, sections};
    use crate::settings::{self, GLASS_STOPS, Tabs};
    use crate::settings_view::general;
    use crate::test_support::init_settings;
    use crate::theme;

    /// The Layout section's rows come in `docs/layout.md`'s order, wherever the tabs are.
    #[gpui_kit::test]
    fn the_layout_rows_come_in_the_specs_order_wherever_the_tabs_are(cx: &mut TestAppContext) {
        cx.update(init_settings);
        for tabs in Tabs::ALL {
            cx.update(|cx| settings::update(cx, |settings| settings.tabs = tabs));
            let titles = cx.read(|cx| {
                (layout(cx).titles().into_iter())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });
            let order = [
                "Tabs",
                "Address bar",
                "Changes and Files",
                "Unpinned pages",
                "Sidebar glass",
            ];
            assert_eq!(titles, order, "{tabs:?}");
        }
    }

    /// Appearance has one Layout section, the layout's settings in it, and General none of
    /// them; and the sidebar's glass leaves the card alone, the theme's own wherever it's set.
    #[gpui_kit::test]
    fn the_layout_and_its_sidebars_rows_show_together_and_leave_the_card_as_the_theme_has_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::test_support::init);
        let own = cx.read(|cx| theme::palette(cx).card);
        for at in GLASS_STOPS {
            cx.update(|cx| settings::update(cx, |settings| settings.sidebar_glass = at));
            cx.read(|cx| {
                assert_eq!(theme::palette(cx).card, own, "at {at}");
                let rows = [
                    "layout",
                    "tabs",
                    "address bar",
                    "changes and files",
                    "unpinned pages",
                    "sidebar glass",
                ];
                let together = sections(cx)
                    .iter()
                    .filter(|section| rows.iter().all(|row| section.matches(row)))
                    .count();
                assert_eq!(together, 1, "at {at}");
                let on_general = general::sections(cx)
                    .iter()
                    .any(|section| section.matches("layout") || section.matches("unpinned"));
                assert!(!on_general, "at {at}");
            });
        }
    }
}
