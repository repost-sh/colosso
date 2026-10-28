//! General: how the sidebar opens, which layout holds it, and how wide it is.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::switch::Switch;
use gpui_kit::{App, IntoElement, ParentElement as _, Styled as _, div};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use crate::settings::{self, Layout};
use crate::theme;

pub const SEARCH_TERMS: &[&str] = &[
    "Sidebar",
    "Show sidebar on launch",
    "Layout",
    "Orca",
    "Arc",
    "Sidebar width",
];

pub fn sections(cx: &App) -> [impl IntoElement; 1] {
    let p = theme::palette(cx);
    let current = settings::get(cx);
    let layout = current.layout;
    let width = current.sidebar_width.round();
    [SettingsSection::new("Sidebar")
        .child(
            SettingRow::new("Show sidebar on launch")
                .description("Open the window with the sidebar out. ⌘S slides it in and out.")
                .control(
                    Switch::new("sidebar-open-on-launch")
                        .checked(current.sidebar_open_on_launch)
                        .color(p.accent)
                        .on_click(|open, _, cx| {
                            settings::update(cx, |settings| {
                                settings.sidebar_open_on_launch = *open
                            })
                        }),
                ),
        )
        .child(
            SettingRow::new("Layout")
                .description(
                    "Orca keeps the sidebar and tabs inside the card. Arc lists tabs on the glass.",
                )
                .control(MenuButton::new("layout", layout.label(), |cx| {
                    let current = settings::get(cx).layout;
                    Layout::ALL.into_iter().fold(NativeMenu::new(), |menu, layout| {
                        menu.menu_with_check(layout.label(), layout == current, layout.action())
                    })
                })),
        )
        .child(
            SettingRow::new("Sidebar width")
                .description("Drag the sidebar's right edge to resize it.")
                .control(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(format!("{width} pt")),
                )
                .control(
                    PillButton::new("reset-sidebar-width", "Reset")
                        .disabled(width == theme::SIDEBAR_WIDTH)
                        .on_click(|_, _, cx| {
                            settings::update(cx, |settings| {
                                settings.sidebar_width = theme::SIDEBAR_WIDTH
                            })
                        }),
                ),
        )]
}
