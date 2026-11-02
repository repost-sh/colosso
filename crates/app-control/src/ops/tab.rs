//! A worktree's tabs: list, open, select, move, close, reopen. Each tab holds panes, laid out
//! by splits (`ops::pane`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{
    Closed, NoInput, PaneId, PaneInfo, PaneKind, PanePlace, PaneSpec, PaneTree, count,
};
use crate::{ControlError, Hints, Operation, Scope};

/// Whose tabs to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabListInput {
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Every worktree's tabs instead of one's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

/// One tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    /// Its place among the worktree's tabs, from 0.
    pub index: u32,
    /// Whether the worktree shows it.
    pub selected: bool,
    /// How its panes are laid out.
    pub layout: PaneTree,
    /// Its panes.
    pub panes: Vec<PaneInfo>,
}

/// A worktree's tabs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeTabs {
    /// The worktree's checkout path.
    pub path: String,
    /// Its tabs, in order.
    pub tabs: Vec<TabInfo>,
}

/// The tabs `tab_list` found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabListOutput {
    /// Each worktree listed, with its tabs.
    pub worktrees: Vec<WorktreeTabs>,
}

/// `tab_list`: tabs, panes and their ids.
#[derive(Debug)]
pub struct TabList;

impl Operation for TabList {
    const NAME: &'static str = "tab_list";
    const TITLE: &'static str = "List tabs";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A worktree's tabs (or every worktree's, with `all`), each with its split layout and panes: id, kind, title, address, file, unsaved edits, terminal handle, focus. Pane ids from here address every pane and browser tool.";
    const HINTS: Hints = Hints::READ;
    type Input = TabListInput;
    type Output = TabListOutput;

    fn summary(output: &TabListOutput) -> String {
        let tabs: usize = output.worktrees.iter().map(|w| w.tabs.len()).sum();
        let panes: usize = (output.worktrees.iter())
            .flat_map(|w| &w.tabs)
            .map(|t| t.panes.len())
            .sum();
        format!(
            "{} with {} in {}",
            count(tabs, "tab", "tabs"),
            count(panes, "pane", "panes"),
            count(output.worktrees.len(), "worktree", "worktrees")
        )
    }
}

/// Where a new tab goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// After the last tab.
    End,
    /// Right after the selected tab.
    Beside,
}

/// The tab to open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabOpenInput {
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// What the tab shows: `terminal`, `browser`, `editor`, `changes` or `chat`.
    pub kind: PaneKind,
    /// A browser tab's address; a blank page when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// An editor tab's file, absolute or relative to the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The line to show in the file, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column on that line, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// An existing terminal's handle (`term_…`) to reveal instead of starting a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// A new terminal's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A new terminal's command line; the login shell when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// `end` (the default) or `beside` the selected tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    /// Show the new tab; true when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

impl TabOpenInput {
    /// What the new pane shows.
    pub fn spec(&self) -> PaneSpec {
        PaneSpec {
            kind: self.kind,
            url: self.url.clone(),
            file: self.file.clone(),
            line: self.line,
            column: self.column,
            terminal: self.terminal.clone(),
            name: self.name.clone(),
            command: self.command.clone(),
        }
    }
}

/// `tab_open`: a new tab of a kind.
#[derive(Debug)]
pub struct TabOpen;

impl Operation for TabOpen {
    const NAME: &'static str = "tab_open";
    const TITLE: &'static str = "Open tab";
    const SUBJECT: &'static [&'static str] = &["kind", "url", "file", "terminal"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Opens a tab of a kind as the menu would: a terminal (new, or an existing one by `terminal` handle), a browser page at `url`, a `file` in an editor at `line`, the changes, or a chat. Answers the new pane's id and tab.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = TabOpenInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Opened {}", output.summary())
    }
}

/// The tab to select: by a pane in it, or by its place in a worktree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabSelectInput {
    /// A pane of the tab, from `tab_list`. Give this or `index`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// With `index`: the worktree; yours, else the selected one, when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// The tab's place in the worktree, from 0. Give this or `pane`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// Which tab [`TabSelectInput`] means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabTarget {
    /// The tab holding this pane.
    Pane(PaneId),
    /// The tab at `index` in `worktree` (the default one when `None`).
    Index {
        worktree: Option<String>,
        index: u32,
    },
}

impl TabSelectInput {
    /// The tab meant.
    ///
    /// # Errors
    /// `invalid_input` unless exactly one of `pane` and `index` is given, and `worktree`
    /// only with `index`.
    pub fn target(&self) -> Result<TabTarget, ControlError> {
        match (self.pane, self.index, &self.worktree) {
            (Some(pane), None, None) => Ok(TabTarget::Pane(pane)),
            (None, Some(index), worktree) => Ok(TabTarget::Index {
                worktree: worktree.clone(),
                index,
            }),
            _ => Err(ControlError::invalid_input(
                "Give either `pane`, or `index` with an optional `worktree`.",
            )),
        }
    }
}

/// `tab_select`: show a tab.
#[derive(Debug)]
pub struct TabSelect;

impl Operation for TabSelect {
    const NAME: &'static str = "tab_select";
    const TITLE: &'static str = "Select tab";
    const SUBJECT: &'static [&'static str] = &["pane", "index"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Shows a tab: the one holding `pane`, or the one at `index` in a worktree.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = TabSelectInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Showing {}", output.summary())
    }
}

/// The tab to move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabMoveInput {
    /// A pane of the tab, from `tab_list`.
    pub pane: PaneId,
    /// The tab's place after the move, from 0; past the end means last.
    pub to: u32,
}

/// `tab_move`: reorder tabs.
#[derive(Debug)]
pub struct TabMove;

impl Operation for TabMove {
    const NAME: &'static str = "tab_move";
    const TITLE: &'static str = "Move tab";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Moves the tab holding `pane` to another place among its worktree's tabs.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = TabMoveInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Moved to {}", output.summary())
    }
}

/// The tab or pane to close.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloseInput {
    /// The pane, from `tab_list`.
    pub pane: PaneId,
    /// Close even with unsaved edits, discarding them. Needs the destructive scope, and the
    /// person confirms it in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discard: Option<bool>,
}

impl CloseInput {
    /// Whether the call may discard unsaved edits.
    pub fn discards(&self) -> bool {
        self.discard == Some(true)
    }
}

/// `tab_close`: close a tab and its panes.
#[derive(Debug)]
pub struct TabClose;

impl Operation for TabClose {
    const NAME: &'static str = "tab_close";
    const TITLE: &'static str = "Close tab";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const ALSO: &'static [Scope] = &[Scope::Destructive];
    const DESCRIPTION: &'static str = "Closes the tab holding `pane`, with all its panes. Terminals keep running in the daemon. Refuses with unsaved_changes, listing the files, when an editor in it has unsaved edits, unless `discard: true`, which needs the destructive scope and the person's confirmation.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = CloseInput;
    type Output = Closed;

    fn summary(output: &Closed) -> String {
        output.summary("tab")
    }

    fn needs_also(input: &CloseInput) -> bool {
        input.discards()
    }
}

/// `tab_reopen`: the latest closed tab back.
#[derive(Debug)]
pub struct TabReopen;

impl Operation for TabReopen {
    const NAME: &'static str = "tab_reopen";
    const TITLE: &'static str = "Reopen closed tab";
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Reopens the tab closed last, as ⇧⌘T does.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = NoInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Reopened {}", output.summary())
    }
}
