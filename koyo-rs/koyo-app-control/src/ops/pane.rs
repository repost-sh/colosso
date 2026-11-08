//! The panes of a tab: splitting, focusing, resizing, detaching and closing them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::browser::OpenViewport;
use super::shared::{Closed, Edge, PaneId, PaneKind, PanePlace, PaneSpec};
use super::tab::CloseInput;
use crate::{ControlError, Hints, Operation, Scope};

/// The split to make: beside which pane, on which edge, and what goes there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaneSplitInput {
    /// The pane to split, from `tab_list`.
    pub pane: PaneId,
    /// The side of `pane` the other pane goes on.
    pub edge: Edge,
    /// An existing pane to move there, taken from its tab. Give this or `kind`.
    #[serde(rename = "move", default, skip_serializing_if = "Option::is_none")]
    pub move_pane: Option<PaneId>,
    /// What a new pane shows: `terminal`, `browser`, `editor`, `changes` or `chat`. Give this
    /// or `move`; the fields below go with it, as in `tab_open`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<PaneKind>,
    /// A new browser pane's address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A new browser page's viewport as it opens, as in `tab_open`: `{ width, height }`,
    /// `{ device }` or `{ fill: true }`. Left out, the person's default: responsive mode at
    /// 1440 × 900 unless they changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<OpenViewport>,
    /// A new editor pane's file, absolute or relative to the worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// The line to show in the file, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column on that line, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// An existing terminal's handle (`term_…`) to show instead of starting a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// A new terminal's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A new terminal's command line; the login shell when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Show the tab with the split, switching the window to its worktree. Left out, the
    /// person's choice in Settings → MCP server decides, and by default the window stays
    /// where it was. Pass true only when the person asked to see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

/// What goes into a split.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitSource {
    /// An existing pane, moved from its tab.
    Move(PaneId),
    /// A new pane.
    New(PaneSpec),
}

impl PaneSplitInput {
    /// What goes into the split.
    ///
    /// # Errors
    /// `invalid_input` unless exactly one of `move` and `kind` is given, and the new pane's
    /// fields only with `kind`.
    pub fn source(&self) -> Result<SplitSource, ControlError> {
        let extras = self.url.is_some()
            || self.viewport.is_some()
            || self.file.is_some()
            || self.line.is_some()
            || self.column.is_some()
            || self.terminal.is_some()
            || self.name.is_some()
            || self.command.is_some();
        match (self.move_pane, self.kind) {
            (Some(pane), None) if !extras => Ok(SplitSource::Move(pane)),
            (None, Some(kind)) => Ok(SplitSource::New(PaneSpec {
                kind,
                url: self.url.clone(),
                file: self.file.clone(),
                line: self.line,
                column: self.column,
                terminal: self.terminal.clone(),
                name: self.name.clone(),
                command: self.command.clone(),
                viewport: self.viewport.clone(),
            })),
            _ => Err(ControlError::invalid_input(
                "Give either `move` (an existing pane) or `kind` with the new pane's fields.",
            )),
        }
    }
}

/// `pane_split`: a pane beside another.
#[derive(Debug)]
pub struct PaneSplit;

impl Operation for PaneSplit {
    const NAME: &'static str = "pane_split";
    const TITLE: &'static str = "Split pane";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Splits `pane` and puts another pane on its `edge`: an existing one taken from its tab (`move`), or a new one of a `kind` with tab_open's fields; a new browser page opens at 1440 × 900 in responsive mode unless its `viewport` says otherwise, as in tab_open. The window switches to the split's tab only with `select: true`, or with `select` left out when the person chose in Settings to see what agents open. Answers the placed pane's id and tab.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = PaneSplitInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Placed {}", output.summary())
    }
}

/// A pane to act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaneInput {
    /// The pane, from `tab_list`.
    pub pane: PaneId,
}

/// `pane_focus`: give a pane the keyboard.
#[derive(Debug)]
pub struct PaneFocus;

impl Operation for PaneFocus {
    const NAME: &'static str = "pane_focus";
    const TITLE: &'static str = "Focus pane";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Selects the tab holding `pane` and gives that pane the keyboard.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = PaneInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Focused {}", output.summary())
    }
}

/// The share a pane takes of its split.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaneResizeInput {
    /// The pane, from `tab_list`.
    pub pane: PaneId,
    /// The share of its split the pane takes, between 0 and 1; kept within the split's
    /// limits.
    pub ratio: f64,
}

/// A pane's share of its split after a resize.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaneResizeOutput {
    /// The pane.
    pub pane: PaneId,
    /// The share it takes now, after the split's limits.
    pub ratio: f64,
}

/// `pane_resize`: move a split's divider.
#[derive(Debug)]
pub struct PaneResize;

impl Operation for PaneResize {
    const NAME: &'static str = "pane_resize";
    const TITLE: &'static str = "Resize pane";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Gives `pane` a share (`ratio`, 0 to 1) of the split holding it; a pane alone in its tab has no split to resize.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = PaneResizeInput;
    type Output = PaneResizeOutput;

    fn summary(output: &PaneResizeOutput) -> String {
        format!(
            "Pane {} takes {:.0}% of its split",
            output.pane,
            output.ratio * 100.
        )
    }
}

/// `pane_detach`: a pane into a tab of its own.
#[derive(Debug)]
pub struct PaneDetach;

impl Operation for PaneDetach {
    const NAME: &'static str = "pane_detach";
    const TITLE: &'static str = "Detach pane";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Moves `pane` out of its split into a tab of its own, right after its old tab.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = PaneInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Detached {}", output.summary())
    }
}

/// `pane_close`: take one pane out.
#[derive(Debug)]
pub struct PaneClose;

impl Operation for PaneClose {
    const NAME: &'static str = "pane_close";
    const TITLE: &'static str = "Close pane";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Navigate;
    const ALSO: &'static [Scope] = &[Scope::Destructive];
    const DESCRIPTION: &'static str = "Closes one pane, its split's other side taking the room; the last pane closes its tab. Refuses with unsaved_changes when it's an editor with unsaved edits, unless `discard: true`, which needs the destructive scope and the person's confirmation.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = CloseInput;
    type Output = Closed;

    fn summary(output: &Closed) -> String {
        output.summary("pane")
    }

    fn needs_also(input: &CloseInput) -> bool {
        input.discards()
    }
}
