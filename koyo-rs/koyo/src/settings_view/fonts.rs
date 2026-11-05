//! The Appearance page's fonts: the family of the interface, the family of code, and how big a
//! chat's text is.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, SharedString};

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::native_slider::NativeSlider;
use crate::settings::{CHAT_SIZES, SetCodeFont, SetInterfaceFont};
use crate::theme::fonts;

/// What the menus call the two defaults, which a family's own name doesn't say.
const SYSTEM_LABEL: &str = "System (SF Pro)";
const BUNDLED_LABEL: &str = "JetBrains Mono Nerd Font";

pub fn section(cx: &App) -> SettingsSection {
    SettingsSection::new("Fonts")
        .row(
            SettingRow::new("Interface font")
                .description("What everything but code is in.")
                .control(MenuButton::new(
                    "font-interface",
                    label(fonts::interface(cx), fonts::SYSTEM, SYSTEM_LABEL),
                    interface_menu,
                )),
        )
        .row(
            SettingRow::new("Code font")
                .description("What terminals, diffs and code are in.")
                .control(MenuButton::new(
                    "font-code",
                    label(fonts::code(cx), fonts::MONO, BUNDLED_LABEL),
                    code_menu,
                )),
        )
        .row(
            SettingRow::new("Chat text size")
                .description("How big the messages in a chat are, and what you type in one.")
                .control(
                    NativeSlider::new(
                        "chat-text-size",
                        &CHAT_SIZES,
                        |settings| settings.fonts.chat_size(),
                        |settings, size| settings.fonts.chat_size = size,
                    )
                    .between_letters(),
                ),
        )
}

/// The family `in_use`, or what the menus call it if it's the default.
fn label(in_use: &'static str, default: &str, default_label: &'static str) -> SharedString {
    if in_use == default { default_label } else { in_use }.into()
}

fn interface_menu(cx: &App) -> NativeMenu {
    menu(fonts::interface(cx), fonts::SYSTEM, SYSTEM_LABEL, |family| Box::new(SetInterfaceFont(family)), cx)
}

fn code_menu(cx: &App) -> NativeMenu {
    menu(fonts::code(cx), fonts::MONO, BUNDLED_LABEL, |family| Box::new(SetCodeFont(family)), cx)
}

/// The default, then every family there is by name, with the one `in_use` checked. Picking the
/// default sets none rather than its name, so it follows the app's if that ever changes.
fn menu(
    in_use: &str,
    default: &str,
    default_label: &'static str,
    set: fn(Option<String>) -> Box<dyn Action>,
    cx: &App,
) -> NativeMenu {
    let first = NativeMenu::new()
        .menu_with_check(default_label, in_use == default, set(None))
        .separator();
    (fonts::families(cx).iter())
        .filter(|family| *family != default)
        .fold(first, |menu, family| {
            menu.menu_with_check(family.clone(), family == in_use, set(Some(family.clone())))
        })
}
