//! What a page keeps across a restart of the app, as a browser keeps a tab in its session: the
//! pages back and forward of the one it shows, and how far down that one is scrolled. The
//! host says it as it changes ([`crate::HostEvent::Session`]), the app keeps it with the
//! window, and a page brought back opens with it ([`crate::AppRequest::Open`]'s `session`).
//! What a page typed into its forms isn't kept: no engine gives it out (`docs/browser.md` 7).
use serde::{Deserialize, Serialize};

/// A page's history around the one it shows, and that one's scroll.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// The pages before it, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub back: Vec<SessionEntry>,
    /// The pages after it, nearest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward: Vec<SessionEntry>,
    /// How far it's scrolled.
    #[serde(default, skip_serializing_if = "at_top")]
    pub scroll: Scroll,
}

impl Session {
    /// Whether there's nothing to bring back: no page either way, and the top of the page.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.back.is_empty() && self.forward.is_empty() && self.scroll.is_top()
    }
}

/// A page of a history: its address, and its title when it had one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

/// Where a page is scrolled to, in CSS pixels from its top left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scroll {
    pub x: i32,
    pub y: i32,
}

impl Scroll {
    /// Whether it's the page's top left, where a page opens anyway.
    #[must_use]
    pub fn is_top(self) -> bool {
        self.x == 0 && self.y == 0
    }
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if hands the field by reference"
)]
fn at_top(scroll: &Scroll) -> bool {
    scroll.is_top()
}
