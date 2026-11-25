//! The menus of a setting's choices, the one it has checked.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, SharedString};

/// A menu of `choices` with `current` checked, each named by `label` and, picked, setting
/// itself through `action`.
pub fn checked_menu<T: Copy + PartialEq, L: Into<SharedString>>(
    choices: impl IntoIterator<Item = T>,
    current: T,
    label: impl Fn(T) -> L,
    action: impl Fn(T) -> Box<dyn Action>,
) -> NativeMenu {
    checked_items(NativeMenu::new(), choices, current, label, action)
}

/// `menu` with `choices` after what it has, as `checked_menu` lists them.
pub fn checked_items<T: Copy + PartialEq, L: Into<SharedString>>(
    menu: NativeMenu,
    choices: impl IntoIterator<Item = T>,
    current: T,
    label: impl Fn(T) -> L,
    action: impl Fn(T) -> Box<dyn Action>,
) -> NativeMenu {
    choices.into_iter().fold(menu, |menu, choice| {
        menu.menu_with_check(label(choice), choice == current, action(choice))
    })
}
