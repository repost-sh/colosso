//! Appearance: light, dark or the system's, and the gradient glass of each.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, IntoElement, ParentElement as _};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use crate::gradient::{self, GradientTheme};
use crate::picker;
use crate::theme::{self, ThemePreference};

pub const SEARCH_TERMS: &[&str] = &[
    "Theme",
    "System",
    "Light",
    "Dark",
    "Glass",
    "Gradient",
    "Edit theme",
];

pub fn sections(cx: &App) -> [impl IntoElement; 1] {
    let preference = theme::preference(cx);
    let appearance = if cx.theme().mode.is_dark() {
        "dark"
    } else {
        "light"
    };
    let glass_is_default = *gradient::current(cx) == GradientTheme::default();
    [SettingsSection::new("Theme")
        .child(
            SettingRow::new("Appearance")
                .description("Follow the system, or keep Colosso light or dark.")
                .control(MenuButton::new("appearance", preference.label(), |cx| {
                    let current = theme::preference(cx);
                    ThemePreference::ALL
                        .into_iter()
                        .fold(NativeMenu::new(), |menu, option| {
                            menu.menu_with_check(
                                option.label(),
                                option == current,
                                option.action(),
                            )
                        })
                })),
        )
        .child(
            SettingRow::new("Glass")
                .description(format!(
                    "The gradient tinting the window's glass in the {appearance} appearance. \
                     Light and dark each keep their own."
                ))
                .control(
                    PillButton::new("reset-glass", "Reset")
                        .disabled(glass_is_default)
                        .on_click(|_, _, cx| {
                            gradient::update(cx, |glass| *glass = GradientTheme::default())
                        }),
                )
                .control(
                    PillButton::new("edit-glass", "Edit…").on_click(|event, window, cx| {
                        picker::open(event.position(), window, cx)
                    }),
                ),
        )]
}
