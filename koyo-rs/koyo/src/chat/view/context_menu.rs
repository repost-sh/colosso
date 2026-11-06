//! What a right-click on a chat offers (`docs/chat.md` 5.3): copying what's selected, and how
//! the chat looks, which Settings › Chat says too: how wide it is, how big its text is, and how
//! much of the agent's work shows.

use gpui_kit::base::TextSelection;
use gpui_kit::base::input::Copy;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{App, Window};

use crate::settings::{self, chat_size_menu};

/// The menu a right-click on a chat opens, wherever what's under the pointer has none of its own.
pub(super) fn menu(window: &mut Window, cx: &mut App) -> NativeMenu {
    let selected = !TextSelection::selected_text(window, cx).trim().is_empty();
    let settings = settings::get(cx);
    let menu = if selected {
        NativeMenu::new().menu("Copy", Box::new(Copy)).separator()
    } else {
        NativeMenu::new()
    };
    menu.submenu("Width", settings.chat_width.menu())
        .submenu("Text Size", chat_size_menu(settings.fonts.chat_size()))
        .submenu("Verbosity", settings.chat_verbosity.menu())
}
