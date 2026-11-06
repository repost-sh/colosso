//! How wide a chat's column is, and the action its menus set it by (`docs/chat.md` 5.3).

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, Styled, px};
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// How wide the timeline and the composer get: a column that reads easily, a wider one for code
/// and diffs, or as wide as the pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatWidth {
    #[default]
    Narrow,
    Wide,
    Full,
}

/// Sets how wide a chat is, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatWidth(pub ChatWidth);

impl ChatWidth {
    pub const ALL: [ChatWidth; 3] = [ChatWidth::Narrow, ChatWidth::Wide, ChatWidth::Full];

    pub fn label(self) -> &'static str {
        match self {
            ChatWidth::Narrow => "Narrow",
            ChatWidth::Wide => "Wide",
            ChatWidth::Full => "Full width",
        }
    }

    /// The widest the column gets, in pixels; as wide as the pane with none.
    pub fn column(self) -> Option<f32> {
        match self {
            ChatWidth::Narrow => Some(760.),
            ChatWidth::Wide => Some(1080.),
            ChatWidth::Full => None,
        }
    }

    /// `element`, kept to the column: centred in what's wider, it doesn't get wider than it.
    pub fn column_of<E: Styled>(self, element: E) -> E {
        match self.column() {
            Some(width) => element.max_w(px(width)),
            None => element,
        }
    }

    /// The widths, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(ChatWidth::ALL, self, ChatWidth::label, |width| {
            Box::new(SetChatWidth(width))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_width_but_the_full_one_has_a_column_wider_than_the_last() {
        let columns: Vec<_> = ChatWidth::ALL.iter().map(|width| width.column()).collect();
        assert_eq!(columns, [Some(760.), Some(1080.), None]);
        assert_eq!(
            ChatWidth::default(),
            ChatWidth::Narrow,
            "as chats always were"
        );
    }
}
