//! Claude accounts' settings (`docs/claude-accounts.md` 1, 5): whether Koyo manages the Mac's
//! Claude Code login at all, and auto-switch with its threshold.

use gpui_kit::Action;
use koyo_accounts::autoswitch::{DEFAULT_THRESHOLD, clamp_threshold};
use serde::{Deserialize, Serialize};

/// Sets auto-switch's threshold, from its menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetAccountsThreshold(pub f64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccountSettings {
    /// Whether Koyo manages Claude Code's login: off, it never touches it and polls nothing, for
    /// a person who manages accounts with claude-swap or anything else.
    pub manage: bool,
    /// Whether auto-switch moves the Mac's login on as the active account nears its limits:
    /// off by default, as claude-swap's.
    pub auto_switch: bool,
    /// The usage, in percent, at which auto-switch moves on; read through [`Self::threshold`].
    pub threshold: f64,
}

impl AccountSettings {
    /// The threshold held to auto-switch's range, 50-99.9 %, as the engine holds it.
    pub fn threshold(&self) -> f64 {
        clamp_threshold(self.threshold)
    }
}

impl Default for AccountSettings {
    fn default() -> Self {
        AccountSettings {
            manage: true,
            auto_switch: false,
            threshold: DEFAULT_THRESHOLD,
        }
    }
}
