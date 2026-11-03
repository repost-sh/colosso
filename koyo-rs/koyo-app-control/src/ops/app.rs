//! The app and its window: status, focus, layout and panels, the settings pages; and the
//! relay's own `app_launch`, which answers without the app.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{Layout, NoInput, PaneId};
use crate::{Hints, Operation, Scope};

/// The stdio relay's own tool, answered without the app and so not in the
/// [`catalog`](crate::catalog): it starts Koyo and waits up to 30 seconds for its socket.
pub const APP_LAUNCH: &str = "app_launch";
/// [`APP_LAUNCH`]'s title.
pub const APP_LAUNCH_TITLE: &str = "Launch Koyo";
/// [`APP_LAUNCH`]'s description.
pub const APP_LAUNCH_DESCRIPTION: &str = "Starts Koyo when it isn't running and waits up to 30 seconds for it to answer. Call it when another tool fails app_not_running.";

/// The panel on the right of the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RightPanel {
    /// The worktree's changes.
    Changes,
    /// The worktree's files.
    Files,
}

/// The right panel to show, or none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RightPanelChoice {
    Changes,
    Files,
    None,
}

/// A page of Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsPage {
    General,
    Appearance,
    Editor,
    Languages,
    Browser,
    Keyboard,
    Mcp,
    About,
}

/// What the window has selected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    /// The selected project's main checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The selected worktree's checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// The focused pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
}

/// The MCP server's HTTP endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// Whether the HTTP endpoint is on (Settings → MCP server).
    pub http: bool,
    /// Its address, while it's on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// The window's layout and panels.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LayoutState {
    /// `orca` or `arc`.
    pub layout: Layout,
    /// Whether the sidebar is open.
    pub sidebar_open: bool,
    /// The panel open on the right, or null.
    pub right_panel: Option<RightPanel>,
    /// Whether the bottom panel is open.
    pub bottom_panel_open: bool,
}

/// What the app and its window are doing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the window's switches, one field each"
)]
pub struct AppStatusOutput {
    /// Koyo's version.
    pub version: String,
    /// Whether the window is open.
    pub window_open: bool,
    /// Whether the window has the system's focus.
    pub focused: bool,
    /// What the window shows: `workspace`, or `settings:<page>`.
    pub route: String,
    /// `orca` or `arc`.
    pub layout: Layout,
    /// Whether the sidebar is open.
    pub sidebar_open: bool,
    /// The panel open on the right, or null.
    pub right_panel: Option<RightPanel>,
    /// Whether the bottom panel is open.
    pub bottom_panel_open: bool,
    /// What is selected.
    pub selected: Selected,
    /// The MCP server's HTTP endpoint.
    pub mcp: McpStatus,
    /// The app's data folder.
    pub data_dir: String,
}

fn status_summary(output: &AppStatusOutput) -> String {
    let window = match (output.window_open, output.focused) {
        (false, _) => "window closed",
        (true, false) => "window open",
        (true, true) => "window focused",
    };
    let shown = output
        .selected
        .worktree
        .as_deref()
        .or(output.selected.project.as_deref())
        .unwrap_or("nothing selected");
    format!(
        "Koyo {}, {window}, showing {} ({shown})",
        output.version, output.route
    )
}

/// `app_status`: what the window shows.
#[derive(Debug)]
pub struct AppStatus;

impl Operation for AppStatus {
    const NAME: &'static str = "app_status";
    const TITLE: &'static str = "Read app status";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "What Koyo's window shows: its version, whether the window is open and focused, the route (workspace or a settings page), layout and panels, the selected project, worktree and pane, the MCP server's address and the data folder. Start here.";
    const HINTS: Hints = Hints::READ;
    type Input = NoInput;
    type Output = AppStatusOutput;

    fn summary(output: &AppStatusOutput) -> String {
        status_summary(output)
    }
}

/// `app_focus`: bring the window forward.
#[derive(Debug)]
pub struct AppFocus;

impl Operation for AppFocus {
    const NAME: &'static str = "app_focus";
    const TITLE: &'static str = "Focus the window";
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Brings Koyo's window forward, opening it when it's closed. Answers the app's status after.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = NoInput;
    type Output = AppStatusOutput;

    fn summary(output: &AppStatusOutput) -> String {
        status_summary(output)
    }
}

/// What `layout_set` changes; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutSetInput {
    /// Open (true) or close (false) the sidebar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar: Option<bool>,
    /// The panel on the right: `changes`, `files`, or `none` to close it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_panel: Option<RightPanelChoice>,
    /// Open (true) or close (false) the bottom panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_panel: Option<bool>,
    /// The window's layout: `orca` or `arc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
}

/// `layout_set`: the layout and panels.
#[derive(Debug)]
pub struct LayoutSet;

impl Operation for LayoutSet {
    const NAME: &'static str = "layout_set";
    const TITLE: &'static str = "Set the layout";
    const SUBJECT: &'static [&'static str] = &["layout", "rightPanel"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Opens or closes the sidebar, the right panel (changes or files) and the bottom panel, and switches the layout between orca and arc. Fields left out stay as they are.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = LayoutSetInput;
    type Output = LayoutState;

    fn summary(output: &LayoutState) -> String {
        let right = match output.right_panel {
            Some(RightPanel::Changes) => "changes",
            Some(RightPanel::Files) => "files",
            None => "no",
        };
        format!(
            "Layout {}: sidebar {}, {right} right panel, bottom panel {}",
            match output.layout {
                Layout::Orca => "orca",
                Layout::Arc => "arc",
            },
            if output.sidebar_open {
                "open"
            } else {
                "closed"
            },
            if output.bottom_panel_open {
                "open"
            } else {
                "closed"
            },
        )
    }
}

/// Which settings page to open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsOpenInput {
    /// The page; the one last shown, else `general`, when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<SettingsPage>,
}

/// `settings_open`: show Settings.
#[derive(Debug)]
pub struct SettingsOpen;

impl Operation for SettingsOpen {
    const NAME: &'static str = "settings_open";
    const TITLE: &'static str = "Open settings";
    const SUBJECT: &'static [&'static str] = &["page"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Shows Settings in the window, on a page: general, appearance, editor, languages, browser, keyboard, mcp or about. Only shows them; settings_set changes them.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = SettingsOpenInput;
    type Output = AppStatusOutput;

    fn summary(output: &AppStatusOutput) -> String {
        status_summary(output)
    }
}

/// `settings_close`: back to the workspace.
#[derive(Debug)]
pub struct SettingsClose;

impl Operation for SettingsClose {
    const NAME: &'static str = "settings_close";
    const TITLE: &'static str = "Close settings";
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str = "Leaves Settings for the workspace.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = NoInput;
    type Output = AppStatusOutput;

    fn summary(output: &AppStatusOutput) -> String {
        status_summary(output)
    }
}
