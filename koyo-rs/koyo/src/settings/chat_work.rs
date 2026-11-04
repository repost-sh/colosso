//! How much of what an agent did along the way shows in a chat (`docs/chat.md` 5.3). What it said
//! always shows, whichever this is.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::checked_menu;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatWork {
    /// Once a turn is over, all its work behind one line.
    Turns,
    /// A line for each step, which opens on what it gave back.
    #[default]
    Steps,
    /// Every step open: all it gave back, and all the thinking.
    Everything,
}

/// Sets how much of the work shows in a chat, from the Chat settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatWork(pub ChatWork);

impl ChatWork {
    pub const ALL: [ChatWork; 3] = [ChatWork::Turns, ChatWork::Steps, ChatWork::Everything];

    pub fn label(self) -> &'static str {
        match self {
            ChatWork::Turns => "One line per turn",
            ChatWork::Steps => "One line per step",
            ChatWork::Everything => "All output and thinking",
        }
    }

    /// A menu of how much can show, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(ChatWork::ALL, self, ChatWork::label, |work| Box::new(SetChatWork(work)))
    }
}
