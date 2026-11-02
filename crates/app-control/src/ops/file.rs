//! Files in editors and the worktree's changes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{Edge, PaneId, PanePlace, count};
use crate::{ControlError, Hints, Operation, Scope};

/// The file to open, and where.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileOpenInput {
    /// The file, absolute or relative to the worktree.
    pub path: String,
    /// The line to show, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column on that line, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Open it in the preview tab the next file picked takes over, as a click in the tree
    /// does; false opens a tab of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<bool>,
    /// Open it in a split on this side of the focused pane instead of a tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<Edge>,
}

/// `file_open`: a file in an editor.
#[derive(Debug)]
pub struct FileOpen;

impl Operation for FileOpen {
    const NAME: &'static str = "file_open";
    const TITLE: &'static str = "Open file";
    const SUBJECT: &'static [&'static str] = &["path"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Opens a file in an editor tab (or a split, with `split`) at a line and column, showing the tab its editor already has when it's open. Doesn't change the file.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = FileOpenInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Opened the file in {}", output.summary())
    }
}

/// What to save: one editor, or every one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileSaveInput {
    /// The editor's pane, from `tab_list`. Give this or `all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Save every editor in the window with unsaved edits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

/// Which editors [`FileSaveInput`] means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveTarget {
    /// The editor in this pane.
    Pane(PaneId),
    /// Every editor with unsaved edits.
    All,
}

impl FileSaveInput {
    /// The editors meant.
    ///
    /// # Errors
    /// `invalid_input` unless exactly one of `pane` and `all: true` is given.
    pub fn target(&self) -> Result<SaveTarget, ControlError> {
        match (self.pane, self.all) {
            (Some(pane), None | Some(false)) => Ok(SaveTarget::Pane(pane)),
            (None, Some(true)) => Ok(SaveTarget::All),
            _ => Err(ControlError::invalid_input(
                "Give either `pane` (an editor) or `all: true`.",
            )),
        }
    }
}

/// What `file_save` wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSaveOutput {
    /// The files written.
    pub saved: Vec<String>,
}

/// `file_save`: write editors' edits to disk.
#[derive(Debug)]
pub struct FileSave;

impl Operation for FileSave {
    const NAME: &'static str = "file_save";
    const TITLE: &'static str = "Save files";
    const SUBJECT: &'static [&'static str] = &["pane", "all"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Writes an editor's unsaved edits to disk (`pane`), or every editor's in the window (`all: true`). Answers the files written.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = FileSaveInput;
    type Output = FileSaveOutput;

    fn summary(output: &FileSaveOutput) -> String {
        if output.saved.is_empty() {
            "Nothing to save".into()
        } else {
            format!("Saved {}", count(output.saved.len(), "file", "files"))
        }
    }
}

/// Whose changes to show.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangesOpenInput {
    /// The worktree: a checkout path, a branch name, or `current`. Left out, yours, else the
    /// selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
}

/// `changes_open`: the Changes tab.
#[derive(Debug)]
pub struct ChangesOpen;

impl Operation for ChangesOpen {
    const NAME: &'static str = "changes_open";
    const TITLE: &'static str = "Open changes";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Opens a worktree's Changes tab, the diff of what it changed, or shows it when it's open.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = ChangesOpenInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Showing the changes in {}", output.summary())
    }
}
