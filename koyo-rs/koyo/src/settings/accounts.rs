//! Claude accounts' settings (`docs/claude-accounts.md` 1): whether Koyo manages the Mac's Claude
//! Code login at all.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountSettings {
    /// Whether Koyo manages Claude Code's login: off, it never touches it and polls nothing, for
    /// a person who manages accounts with claude-swap or anything else.
    pub manage: bool,
}

impl Default for AccountSettings {
    fn default() -> Self {
        AccountSettings { manage: true }
    }
}
