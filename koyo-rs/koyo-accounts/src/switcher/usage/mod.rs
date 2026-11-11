//! Port of claude-swap's `switcher.py` (0fe22a8), usage collection: the store-backed entries
//! the app and the auto-switch engine read (`usage_entries_by_account`), the lines and models
//! poll planning keys on, and the parts in this folder: the active account's fetch
//! (`active.rs`) and its rotated-backup resync (`resync.rs`), the idle accounts' fetch and the
//! fetch pool (`idle.rs`), and one collection pass with its poll plans (`collect.rs`).
//!
//! The spec's rule, where Koyo differs from cswap: never a token refresh to read usage, as
//! Orca; renewal happens only through the refresh gate (`docs/claude-accounts.md` 4). cswap
//! refreshes an expired active token under Claude Code's lock protocol and an expired idle one
//! through the gate; here neither is refreshed, and an expired token is the `token expired`
//! sentinel.
//!
//! `_poll_policy_inputs` reads cswap's settings file; Koyo has none, so the inputs are cswap's
//! defaults (threshold 90, no weekly threshold, no models) until the engine pins its own with
//! [`Switcher::set_poll_policy_inputs`]. Left out: `clear_poll_policy_inputs` (Koyo's engine
//! lives as long as the app, so nothing unpins), `usage_by_account` and `usage_fetch_stamps`
//! (no view of Koyo's reads them), and `_header_probe_enabled` (the header probe isn't ported,
//! and the usage fetch takes no flag for it).

use std::collections::BTreeMap;

use crate::Result;
use crate::switcher::{Switcher, locked};
use crate::usage_store::UsageEntry;

mod active;
mod collect;
mod idle;
mod resync;

pub use collect::Fetch;
#[cfg(test)]
pub(crate) use idle::RUN_FETCHES;

/// cswap's `autoswitch.threshold` default.
pub(crate) const DEFAULT_THRESHOLD: f64 = 90.;

/// The switch lines and model names poll planning keys on, cswap's `(threshold,
/// weekly_threshold, models)`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PollInputs {
    pub threshold: f64,
    pub weekly_threshold: Option<f64>,
    pub models: Vec<String>,
}

impl Default for PollInputs {
    fn default() -> Self {
        PollInputs {
            threshold: DEFAULT_THRESHOLD,
            weekly_threshold: None,
            models: Vec::new(),
        }
    }
}

impl PollInputs {
    pub(crate) fn model_names(&self) -> Vec<&str> {
        self.models.iter().map(String::as_str).collect()
    }
}

impl Switcher {
    /// cswap's `usage_entries_by_account`: the store-backed usage entry (ages, errors, poll
    /// state, the pass's sentinel) of every managed account, after one collection pass in
    /// which `fetch` decides what may be fetched.
    ///
    /// # Errors
    /// As [`Switcher::roster`], and the usage store's lock or write.
    pub fn usage_entries_by_account(
        &self,
        fetch: Fetch<'_>,
    ) -> Result<BTreeMap<String, UsageEntry>> {
        let infos = self.build_accounts_info()?;
        self.collect_usage_entries(&infos, fetch)
    }

    /// cswap's `set_poll_policy_inputs`: pins the lines and models poll planning keys on, so
    /// the cadence follows the engine's effective settings.
    pub fn set_poll_policy_inputs(
        &self,
        threshold: f64,
        weekly_threshold: Option<f64>,
        models: &[String],
    ) {
        *locked(&self.poll_inputs) = PollInputs {
            threshold,
            weekly_threshold,
            models: models.to_vec(),
        };
    }

    /// cswap's `_poll_policy_inputs`: the pinned inputs, else cswap's defaults. Read once per
    /// pass, so one pass plans on one set.
    pub(crate) fn poll_policy_inputs(&self) -> PollInputs {
        locked(&self.poll_inputs).clone()
    }
}
