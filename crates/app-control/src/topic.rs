//! What a client can follow (`docs/app-control.md` 7). Each [`Topic`] stands for the results
//! of a few reads; an event about it says only that they changed, so the client reads again.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Scope;

/// Something whose changes a client can follow.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// The window, as `app_status` reads it (`colosso://app`).
    App,
    /// Projects, worktrees, tabs and panes, as `project_list` and `tab_list` read them
    /// (`colosso://projects`, `colosso://tabs`).
    Workspace,
    /// The allowlisted settings, as `settings_get` reads them (`colosso://settings`).
    Settings,
}

impl Topic {
    /// Every topic.
    pub const ALL: [Topic; 3] = [Topic::App, Topic::Workspace, Topic::Settings];

    /// The scope following any topic needs.
    pub const SCOPE: Scope = Scope::Read;

    /// The wire identifier, as serialized.
    pub fn id(self) -> &'static str {
        match self {
            Topic::App => "app",
            Topic::Workspace => "workspace",
            Topic::Settings => "settings",
        }
    }

    /// The read operations whose results this topic covers.
    pub fn operations(self) -> &'static [&'static str] {
        match self {
            Topic::App => &["app_status"],
            Topic::Workspace => &["project_list", "tab_list"],
            Topic::Settings => &["settings_get"],
        }
    }

    /// The MCP resources this topic covers.
    pub fn resources(self) -> &'static [&'static str] {
        match self {
            Topic::App => &["colosso://app"],
            Topic::Workspace => &["colosso://projects", "colosso://tabs"],
            Topic::Settings => &["colosso://settings"],
        }
    }
}
