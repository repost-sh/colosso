//! Types more than one domain uses: panes and their layout, projects and worktrees, where a
//! pane landed, and what closing discarded.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A pane's id: stable while the app runs, listed by `tab_list`.
pub type PaneId = u64;

/// The input of an operation that takes no arguments.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoInput {}

/// What a pane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    /// A terminal the daemon runs.
    Terminal,
    /// A browser page.
    Browser,
    /// A file in an editor.
    Editor,
    /// The worktree's changes.
    Changes,
    /// A chat.
    Chat,
}

impl PaneKind {
    /// The wire identifier, as serialized.
    pub fn id(self) -> &'static str {
        match self {
            PaneKind::Terminal => "terminal",
            PaneKind::Browser => "browser",
            PaneKind::Editor => "editor",
            PaneKind::Changes => "changes",
            PaneKind::Chat => "chat",
        }
    }
}

/// Which way a split lays its two sides out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Side by side.
    Row,
    /// One above the other.
    Column,
}

/// A side of a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The window's layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// The sidebar of projects beside the content.
    Orca,
    /// Tabs down the side, as Arc has them.
    Arc,
}

/// One pane of a tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaneInfo {
    /// The pane's id.
    pub pane: PaneId,
    /// What it shows.
    pub kind: PaneKind,
    /// Its title, as its tab shows it.
    pub title: String,
    /// A browser pane's address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// An editor's file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Whether an editor holds unsaved edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsaved: Option<bool>,
    /// A terminal pane's handle (`term_…`), for the `terminal_*` tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// Whether it has the keyboard.
    pub focused: bool,
}

/// How a tab's panes are laid out: a pane, or a split of two layouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PaneTree {
    /// A split: `first` left of or above `second`.
    Split {
        /// Which way the sides are laid out.
        axis: Axis,
        /// The share of the split `first` takes, between 0 and 1.
        ratio: f64,
        /// The left or top side.
        first: Box<PaneTree>,
        /// The right or bottom side.
        second: Box<PaneTree>,
    },
    /// One pane.
    Pane {
        /// The pane's id.
        pane: PaneId,
    },
}

/// A worktree of a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// The checkout's path.
    pub path: String,
    /// The branch checked out; absent when detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Whether `HEAD` is detached.
    #[serde(default, skip_serializing_if = "is_false")]
    pub detached: bool,
    /// Whether it's the project's main checkout.
    pub main: bool,
    /// Whether the window shows it.
    pub selected: bool,
    /// How many tabs it has open.
    pub tabs: u32,
    /// How many terminals it has.
    pub terminals: u32,
}

/// A project in the sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    /// The main checkout's path.
    pub path: String,
    /// Its name, as the sidebar shows it.
    pub name: String,
    /// Whether the window shows it.
    pub selected: bool,
    /// Why it can't be read, when it can't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Its worktrees, the main checkout first.
    pub worktrees: Vec<WorktreeInfo>,
}

/// Where a pane is: its worktree, its tab and itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PanePlace {
    /// The pane's id.
    pub pane: PaneId,
    /// Its tab's index in the worktree, from 0.
    pub tab: u32,
    /// The worktree's checkout path.
    pub worktree: String,
}

impl PanePlace {
    /// "pane 7 in tab 2 of /path".
    pub fn summary(&self) -> String {
        format!(
            "pane {} in tab {} of {}",
            self.pane, self.tab, self.worktree
        )
    }
}

/// What closing a tab or a pane did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    /// Whether something closed; false when it was gone already.
    pub closed: bool,
    /// The files whose unsaved edits were discarded.
    #[serde(default)]
    pub discarded: Vec<String>,
}

impl Closed {
    pub fn summary(&self, what: &str) -> String {
        match (self.closed, self.discarded.len()) {
            (false, _) => format!("No {what} to close"),
            (true, 0) => format!("Closed the {what}"),
            (true, files) => {
                format!("Closed the {what}, discarding unsaved edits in {files} files")
            }
        }
    }
}

/// What a new pane shows, from `tab_open`'s or `pane_split`'s fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneSpec {
    pub kind: PaneKind,
    pub url: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub terminal: Option<String>,
    pub name: Option<String>,
    pub command: Option<String>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

/// "1 project" or "3 projects".
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}
