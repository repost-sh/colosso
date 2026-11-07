//! What the composer's edge does while a chat works (`docs/chat.md` 5.3): a short, soft line going
//! slowly round it, a brighter one, the edge only a little brighter, or nothing more than any
//! glass's faint edge.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// How the composer shows that the agent, or one of its subagents, is working.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatActivity {
    /// A short, soft line goes slowly round the composer's edge.
    #[default]
    Subtle,
    /// The same line, brighter and a little longer.
    Clear,
    /// The edge is a little brighter all round, and holds still.
    Still,
    /// The edge stays as faint as any glass's.
    Off,
}

/// Sets what the composer's edge does while a chat works, from the Chat settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatActivity(pub ChatActivity);

impl ChatActivity {
    pub const ALL: [ChatActivity; 4] = [
        ChatActivity::Subtle,
        ChatActivity::Clear,
        ChatActivity::Still,
        ChatActivity::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ChatActivity::Subtle => "Subtle line",
            ChatActivity::Clear => "Clear line",
            ChatActivity::Still => "Still edge",
            ChatActivity::Off => "Off",
        }
    }

    /// The choices, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(ChatActivity::ALL, self, ChatActivity::label, |activity| {
            Box::new(SetChatActivity(activity))
        })
    }
}
