//! A page's `DevTools` through their own controls (`docs/browser.md` 13): the side their
//! frontend opens at, and what it asks of the app from its dock menu, its × and its device
//! toolbar toggle.
use serde::{Deserialize, Serialize};

/// Where a page's `DevTools` are, by the frontend's names: docked at a side of the page, or
/// undocked, which in the app is a pane of their own beside the page's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DevToolsDock {
    /// At the bottom of the page.
    #[default]
    Bottom,
    /// Left of the page.
    Left,
    /// Right of the page.
    Right,
    /// Apart from the page.
    Undocked,
}

/// What a `DevTools` frontend asks from its own controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DevToolsAsk {
    /// To be at a side, or undocked: picked in its menu's Dock side, or ⇧⌘D.
    Dock {
        /// Where.
        side: DevToolsDock,
    },
    /// To close: its ×.
    Close,
    /// Its device toolbar toggle turned on or off.
    DeviceMode {
        /// Whether it's on.
        on: bool,
    },
}
