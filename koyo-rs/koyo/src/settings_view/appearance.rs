//! Appearance: light, dark or the system's, and the gradient glass of each; the layout: where the
//! tabs, the address bar and the Changes and Files buttons go, whether the unpinned pages are each
//! space's or shared, and how far in the sidebar's rows step (`docs/layout.md`,
//! Settings); the wallpaper behind chats; and the fonts.

use gpui_kit::App;
use gpui_kit::component::ActiveTheme as _;

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection, slider};
use super::native_slider::NativeSlider;
use super::{chat_backdrop, fonts};
use crate::gradient::{self, GradientTheme};
use crate::picker;
use crate::settings::{self, INDENT_STOPS, Settings};
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
/// Files buttons go, the unpinned pages, the sidebar's indentation.
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
        .row(sidebar_indent_row())
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

/// The row for how far in the sidebar's rows step.
fn sidebar_indent_row() -> SettingRow {
    let indent = NativeSlider::new(
        "sidebar-indent",
        &INDENT_STOPS,
        |settings| settings.sidebar_indent,
        |settings, stop| settings.sidebar_indent = stop,
    );
    slider(
        (
            "Sidebar indentation",
            "How far in a branch sits under its project in the sidebar, and a tab under its \
             branch: the left end is none.",
        ),
        indent,
        ("None", "More"),
    )
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::{layout, sections};
    use crate::settings::{self, Tabs};
    use crate::settings_view::general;
    use crate::test_support::init_settings;

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
                "Sidebar indentation",
            ];
            assert_eq!(titles, order, "{tabs:?}");
        }
    }

    /// Appearance has one Layout section, the layout's settings in it, and General none of
    /// them.
    #[gpui_kit::test]
    fn the_layouts_rows_show_together_in_appearance_alone(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        cx.read(|cx| {
            let rows = [
                "layout",
                "tabs",
                "address bar",
                "changes and files",
                "unpinned pages",
                "sidebar indentation",
            ];
            let together = sections(cx)
                .iter()
                .filter(|section| rows.iter().all(|row| section.matches(row)))
                .count();
            assert_eq!(together, 1);
            let on_general = general::sections(cx)
                .iter()
                .any(|section| section.matches("layout") || section.matches("unpinned"));
            assert!(!on_general);
        });
    }
}
