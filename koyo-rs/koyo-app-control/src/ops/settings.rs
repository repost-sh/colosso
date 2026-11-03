//! The allowlisted settings (`docs/app-control.md` 5, Settings). The list is closed on
//! purpose: the projects (the project tools change them), the control policy
//! (`externalControl`, the person's alone), themes' gradients and language server tables are
//! never read or written here.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use super::shared::{Layout, NoInput};
use crate::{Hints, Operation, Scope};

/// Light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// Follow the system.
    System,
    Light,
    Dark,
}

/// What ⌘S saves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveScope {
    /// The file shown.
    Shown,
    /// Every file with unsaved edits.
    All,
}

/// What new worktrees' branches are named under: `"githubUser"`, `"none"`, or
/// `{"custom": "<prefix>"}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BranchPrefix {
    /// The GitHub username git's config or the GitHub CLI knows; none if neither does.
    GithubUser,
    /// A prefix of one's own.
    Custom(String),
    /// No prefix.
    None,
}

/// Where a page's `DevTools` open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DevtoolsDock {
    Bottom,
    Left,
    Right,
    /// In a pane of their own.
    Detached,
}

/// Where a page's `DevTools` open, and the room they take docked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Devtools {
    /// `bottom`, `left`, `right` or `detached`.
    pub dock: DevtoolsDock,
    /// Their height at the bottom of a page, in points.
    pub height: f64,
    /// Their width at a side of a page, in points.
    pub width: f64,
}

/// A change to where `DevTools` open; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DevtoolsChange {
    /// `bottom`, `left`, `right` or `detached`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dock: Option<DevtoolsDock>,
    /// Their height at the bottom of a page, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    /// Their width at a side of a page, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// The allowlisted settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_excessive_bools,
    reason = "settings: each switch is one field"
)]
pub struct SettingsValues {
    /// `system`, `light` or `dark`.
    pub appearance: Appearance,
    /// The window's layout: `orca` or `arc`.
    pub layout: Layout,
    /// Whether a file picked in the tree opens in a preview tab the next one takes over.
    pub preview_files: bool,
    /// What ⌘S saves: `shown` or `all`.
    pub save_scope: SaveScope,
    /// Whether inferred types and parameter names show greyed in the code.
    pub inlay_hints: bool,
    /// Whether a language server's reading of the code colours it over the grammar's.
    pub semantic_colouring: bool,
    /// Whether the sidebar is open when Koyo starts.
    pub sidebar_open_on_launch: bool,
    /// Where new worktrees go, a folder per project; null for `~/koyo/workspaces`.
    pub worktrees_dir: Option<String>,
    /// What new branches are named under.
    pub branch_prefix: BranchPrefix,
    /// The most language servers running at once; null for no limit.
    pub most_language_servers: Option<u32>,
    /// Whether a language server stops after ten minutes without an open file.
    pub stop_idle_language_servers: bool,
    /// The sidebar's width, in points.
    pub sidebar_width: f64,
    /// Where a page's `DevTools` open.
    pub devtools: Devtools,
}

/// `settings_get`: the allowlisted settings.
#[derive(Debug)]
pub struct SettingsGet;

impl Operation for SettingsGet {
    const NAME: &'static str = "settings_get";
    const TITLE: &'static str = "Read settings";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The settings an agent may read and change: appearance, layout, editor behaviour, worktree naming and folder, language server limits, sidebar width, DevTools placement.";
    const HINTS: Hints = Hints::READ;
    type Input = NoInput;
    type Output = SettingsValues;

    fn summary(output: &SettingsValues) -> String {
        summary(output)
    }
}

/// The settings to change; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsSetInput {
    /// `system`, `light` or `dark`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    /// The window's layout: `orca` or `arc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// Whether a file picked in the tree opens in a preview tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_files: Option<bool>,
    /// What ⌘S saves: `shown` or `all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_scope: Option<SaveScope>,
    /// Whether inferred types and parameter names show greyed in the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlay_hints: Option<bool>,
    /// Whether a language server's reading of the code colours it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_colouring: Option<bool>,
    /// Whether the sidebar is open when Koyo starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_open_on_launch: Option<bool>,
    /// Where new worktrees go, an absolute path; null for `~/koyo/workspaces`.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[expect(
        clippy::option_option,
        reason = "left out keeps it, null resets it, a path sets it"
    )]
    pub worktrees_dir: Option<Option<String>>,
    /// What new branches are named under: `"githubUser"`, `"none"` or `{"custom": "…"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_prefix: Option<BranchPrefix>,
    /// The most language servers running at once, at least 1; null for no limit.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    #[expect(
        clippy::option_option,
        reason = "left out keeps it, null resets it, a number sets it"
    )]
    pub most_language_servers: Option<Option<u32>>,
    /// Whether a language server stops after ten minutes without an open file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_idle_language_servers: Option<bool>,
    /// The sidebar's width, in points; kept within its limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f64>,
    /// Where a page's `DevTools` open, and the room they take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devtools: Option<DevtoolsChange>,
}

/// Reads a field that is present, null included, as `Some`; an absent one stays `None`
/// through `#[serde(default)]`. So `null` resets a setting and leaving it out keeps it.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// `settings_set`: change allowlisted settings.
#[derive(Debug)]
pub struct SettingsSet;

impl Operation for SettingsSet {
    const NAME: &'static str = "settings_set";
    const TITLE: &'static str = "Change settings";
    const SCOPE: Scope = Scope::Settings;
    const DESCRIPTION: &'static str = "Changes the settings settings_get reads; fields left out stay as they are, and null resets `worktreesDir` and `mostLanguageServers`. Projects and the MCP server's own policy can't be changed here. Answers the settings after.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = SettingsSetInput;
    type Output = SettingsValues;

    fn summary(output: &SettingsValues) -> String {
        format!("Settings saved: {}", summary(output))
    }
}

fn summary(output: &SettingsValues) -> String {
    let appearance = match output.appearance {
        Appearance::System => "system",
        Appearance::Light => "light",
        Appearance::Dark => "dark",
    };
    let layout = match output.layout {
        Layout::Orca => "orca",
        Layout::Arc => "arc",
    };
    format!(
        "{appearance} appearance, {layout} layout, worktrees in {}",
        output
            .worktrees_dir
            .as_deref()
            .unwrap_or("~/koyo/workspaces")
    )
}
