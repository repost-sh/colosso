//! What the details card over a chat shows, and whether it's up (`docs/chat.md` 5.3).

use gpui_kit::Action;
use serde::{Deserialize, Serialize};

/// A part of the card a person can do without.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetailsItem {
    Worktree,
    Changes,
    Subagents,
    Context,
}

impl DetailsItem {
    pub const ALL: [DetailsItem; 4] =
        [DetailsItem::Worktree, DetailsItem::Changes, DetailsItem::Subagents, DetailsItem::Context];

    pub fn label(self) -> &'static str {
        match self {
            DetailsItem::Worktree => "Branch and worktree",
            DetailsItem::Changes => "Changes",
            DetailsItem::Subagents => "Subagents",
            DetailsItem::Context => "Context window",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatDetails {
    /// Whether it's up where it has room beside the timeline.
    pub open: bool,
    /// What a person turned off of it.
    pub hidden: Vec<DetailsItem>,
}

impl Default for ChatDetails {
    fn default() -> Self {
        ChatDetails {
            open: true,
            hidden: Vec::new(),
        }
    }
}

impl ChatDetails {
    pub fn shows(&self, item: DetailsItem) -> bool {
        !self.hidden.contains(&item)
    }

    pub fn flip(&mut self, item: DetailsItem) {
        if self.shows(item) {
            self.hidden.push(item);
        } else {
            self.hidden.retain(|hidden| *hidden != item);
        }
    }
}

/// Turns a part of the details card on or off, from its menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct FlipDetailsItem(pub DetailsItem);
