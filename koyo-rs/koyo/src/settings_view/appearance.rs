//! Appearance: light, dark or the system's, and the gradient glass of each; the layout, with how
//! much of the window's glass its sidebar lets through, how blurred the desktop behind the
//! sidebar is, and whether the Arc sidebar's unpinned pages are each space's or shared; the
//! wallpaper behind chats; and the fonts.

use gpui_kit::App;
use gpui_kit::component::ActiveTheme as _;

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection, slider};
use super::native_slider::NativeSlider;
use super::{chat_backdrop, fonts};
use crate::gradient::{self, GradientTheme};
use crate::picker;
use crate::settings::{self, GLASS_STOPS};
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

/// The layout, then what's its sidebar's: how much of the window's glass the sidebar lets
/// through and how blurred the desktop behind it is, in either layout, and the Arc sidebar's
/// unpinned pages.
fn layout(cx: &App) -> SettingsSection {
    let current = settings::get(cx);
    let (layout, loose) = (current.layout, current.loose_pages);
    let [glass, blur] = sidebar_glass_rows();
    SettingsSection::new("Layout")
        .row(
            SettingRow::new("Layout")
                .description(
                    "Orca keeps the sidebar and tabs inside the card. Arc lists tabs on the glass.",
                )
                .control(MenuButton::new("layout", layout.label(), |cx| {
                    settings::get(cx).layout.menu()
                })),
        )
        .row(glass)
        .row(blur)
        .row(
            SettingRow::new("Unpinned pages")
                .description(
                    "In the Arc sidebar, the pages under the pinned ones that are no worktree's. \
                     Per space, each space lists its own; Shared, every space lists them all. A \
                     page opened while they're shared is the space's shown then, so going back \
                     to Per space puts each in its space. Pinned pages are in every space.",
                )
                .control(MenuButton::new("loose-pages", loose.label(), |cx| {
                    settings::get(cx).loose_pages.menu()
                })),
        )
}

/// The rows for the sidebar's glass: how much of the window's glass shows through it, and how
/// blurred the desktop behind it is.
fn sidebar_glass_rows() -> [SettingRow; 2] {
    let glass = NativeSlider::new(
        "sidebar-glass",
        &GLASS_STOPS,
        |settings| settings.sidebar_glass,
        |settings, stop| settings.sidebar_glass = stop,
    );
    let blur = NativeSlider::new(
        "sidebar-blur",
        &GLASS_STOPS,
        |settings| settings.sidebar_blur,
        |settings, stop| settings.sidebar_blur = stop,
    );
    [
        slider(
            (
                "Sidebar glass",
                "How much of the window's glass shows through the sidebar: the left end is \
                 clear, the middle the card's own, the right end solid.",
            ),
            glass,
            ("Clear", "Solid"),
        ),
        slider(
            (
                "Sidebar blur",
                "How blurred the desktop behind the sidebar looks: the middle is macOS's own, \
                 as the rest of the window has it, the left end none.",
            ),
            blur,
            ("None", "More"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::sections;
    use crate::settings::{self, GLASS_STOPS, Layout};
    use crate::settings_view::general;
    use crate::theme;

    /// Appearance has one Layout section, the layout's own settings with it, in either layout,
    /// and General none of them; and the sidebar's glass and blur leave the card alone, the
    /// theme's own wherever they're set.
    #[gpui_kit::test]
    fn the_layout_and_its_sidebars_rows_show_together_and_leave_the_card_as_the_theme_has_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::test_support::init);
        let own = cx.read(|cx| theme::palette(cx).card);
        for layout in Layout::ALL {
            for at in GLASS_STOPS {
                cx.update(|cx| {
                    settings::update(cx, |settings| {
                        (settings.layout, settings.sidebar_glass, settings.sidebar_blur) =
                            (layout, at, at);
                    });
                });
                cx.read(|cx| {
                    assert_eq!(theme::palette(cx).card, own, "{layout:?} at {at}");
                    let rows = ["layout", "sidebar glass", "sidebar blur", "unpinned pages"];
                    let together = sections(cx)
                        .iter()
                        .filter(|section| rows.iter().all(|row| section.matches(row)))
                        .count();
                    assert_eq!(together, 1, "{layout:?}");
                    let on_general = general::sections(cx)
                        .iter()
                        .any(|section| section.matches("layout") || section.matches("unpinned"));
                    assert!(!on_general, "{layout:?}");
                });
            }
        }
    }
}
