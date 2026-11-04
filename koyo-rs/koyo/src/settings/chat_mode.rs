//! What a new chat may do without asking, unless it's left to Claude's own settings.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use koyo_agent::protocol::model::Mode;
use koyo_agent::words::mode_label;

use super::checked_menu;

/// Sets what a new chat may do, from the Chat settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatMode(pub Option<Mode>);

pub fn chat_mode_label(mode: Option<Mode>) -> &'static str {
    mode.map_or("As Claude Code is set", mode_label)
}

/// A menu of what a new chat may do, with `current` checked.
pub fn chat_mode_menu(current: Option<Mode>) -> NativeMenu {
    let choices = std::iter::once(None).chain(Mode::ALL.map(Some));
    checked_menu(choices, current, chat_mode_label, |mode| Box::new(SetChatMode(mode)))
}
