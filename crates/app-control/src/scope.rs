//! The scopes a caller may be granted (`docs/app-control.md` 4). Every operation needs one;
//! a few calls need a second as well, depending on their input (`Operation::ALSO`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a caller is allowed to do, granted per client.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Lists, reads, status, screenshots and resources.
    Read,
    /// Selecting projects and worktrees; opening, selecting, moving and closing tabs;
    /// splitting, focusing and resizing panes; panels, layout, settings pages, opening files.
    Navigate,
    /// Adding, removing and reordering projects, creating worktrees, saving files.
    Manage,
    /// Driving pages: navigating, evaluating, clicking, typing, keys, scrolling, claiming.
    Browser,
    /// `settings_set`, on the allowlisted fields.
    Settings,
    /// Deleting worktrees and closing tabs that discard unsaved edits; the person confirms
    /// each call in the window.
    Destructive,
}

impl Scope {
    /// Every scope, in order of increasing reach.
    pub const ALL: [Scope; 6] = [
        Scope::Read,
        Scope::Navigate,
        Scope::Manage,
        Scope::Browser,
        Scope::Settings,
        Scope::Destructive,
    ];

    /// What an external client the person allowed gets: `settings` and `destructive` stay
    /// off until switched on in Settings.
    pub const DEFAULT_GRANT: [Scope; 4] =
        [Scope::Read, Scope::Navigate, Scope::Manage, Scope::Browser];

    /// What an agent in a Colosso terminal gets without asking: everything but
    /// `destructive`, whose calls the window confirms regardless.
    pub const TERMINAL_AGENT: [Scope; 5] = [
        Scope::Read,
        Scope::Navigate,
        Scope::Manage,
        Scope::Browser,
        Scope::Settings,
    ];

    /// The wire identifier, as serialized.
    pub fn id(self) -> &'static str {
        match self {
            Scope::Read => "read",
            Scope::Navigate => "navigate",
            Scope::Manage => "manage",
            Scope::Browser => "browser",
            Scope::Settings => "settings",
            Scope::Destructive => "destructive",
        }
    }

    /// The scope `id` names, if any.
    pub fn from_id(id: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|scope| scope.id() == id)
    }

    /// What Settings calls it.
    pub fn label(self) -> &'static str {
        match self {
            Scope::Read => "Read",
            Scope::Navigate => "Navigate",
            Scope::Manage => "Manage",
            Scope::Browser => "Browser",
            Scope::Settings => "Settings",
            Scope::Destructive => "Destructive",
        }
    }
}
