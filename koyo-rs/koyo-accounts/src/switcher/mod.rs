//! Port of claude-swap's `switcher.py` (0fe22a8), its account half: the switcher with its
//! store (`sequence.json`, `configs/`), adding the current login, removing an account, holding
//! one out of auto-switch, and switching to a slot with its rollback.
//!
//! - `store.rs`: `sequence.json` and the per-slot config backups, read and written as cswap's.
//! - `identity.rs`: the live login's identity, slot lookup, the per-slot identity accessors.
//! - `policy.rs`: the `disabled` flag that holds a slot out of automatic selection.
//! - `capture.rs`, `add.rs`: adding the current login, with every capture guard.
//! - `remove.rs`: removing a slot.
//! - `switch/`: `switch_to`, the classification of the outgoing credential, the switch itself
//!   with its transaction and rollback, the safety copy and the foreign-successor rescue.
//!
//! Koyo's switcher returns what cswap prints: a switch's result is a [`SwitchResult`] carrying
//! what cswap's `--json` result carries, an add an [`AddResult`], so the app says it. Every
//! warning cswap prints in human mode rides in the result's `warnings`, as in its JSON mode.
//!
//! Left out (`docs/claude-accounts.md` 3): aliases (`set_alias`, `_find_account_by_alias`,
//! `_alias_in_use`, …), `swap_accounts`, `move_account`, priority tiers and per-account
//! thresholds, API-key and setup-token accounts (`add_account_from_token`, the
//! `api-key-mismatch` verdict, `_reject_cross_kind_collision`), session mode and every
//! session-profile check (`_ensure_no_live_session`, `_session_profile_ahead`,
//! `_adopt_session_credential*`, `_invalidate_session_credentials`, `_delete_session_profile`,
//! `_session_dir`, `live_session_pids_for`, `_refuse_session_shell`, and with them
//! `_post_backup_write`, which only invalidates session profiles), process detection,
//! directory mappings (`_prune_mappings`), the bare `switch()` with its rotation, best and
//! next-available strategies (Koyo switches to a named slot only), `--force` activation (no
//! caller of Koyo's asks for it), the `--slot` and `--alias` arguments of `add_account` and the
//! overwrite prompt that goes with them (Koyo adds the current login into a new slot or its
//! own), migrations (`_migrate_org_fields`, `_get_sequence_data_migrated`: Koyo's store starts
//! new), printing (`list_accounts`, `status`, `_print_switch_followup`, display helpers),
//! `purge`, `_first_run_setup` and container detection.
//!
//! The usage half (the refresh gate `consume_backup_grant`, usage collection, `accounts_snapshot`
//! and `_build_accounts_info`) builds on this module and is not ported yet; so neither is what
//! only it uses: the per-thread active verdict (`_record_active_verdict`, `_active_verdict`,
//! `_with_active_verdict`, the `_active_*` flags: the usage half carries the verdict as a value
//! on the active row of the accounts info, as `_build_accounts_info` sets it), the provenance
//! warnings (`_provenance_warned`), the probe that records ownership verdicts
//! (`_resolved_matches_slot_identity` and the verdict write) and `_live_identity_matches`. The
//! verdict cache itself is here, as the switch consults it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::credentials::CredentialStore;
use crate::usage_store::UsageStore;
use crate::{Env, paths};

mod add;
mod capture;
mod identity;
mod policy;
mod remove;
mod store;
mod switch;
#[cfg(test)]
mod tests;

pub use add::AddResult;
pub use identity::SlotIdentity;
pub use policy::HoldOutcome;
pub use remove::RemovedAccount;
pub use switch::{AccountRef, SwitchReason, SwitchResult};

/// A `_probe_verdicts` key (`Switcher::lineage_key`): the slot, the caller's email, the slot's
/// full stored identity and the refresh lineage's fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineageKey {
    pub slot: String,
    pub caller_email: String,
    pub identity: SlotIdentity,
    pub fingerprint: String,
}

/// cswap's `ClaudeAccountSwitcher`: one per process, called from background threads.
///
/// It owns the credential store and the usage store, and the ownership verdicts cswap keeps
/// in process.
pub struct Switcher {
    env: Env,
    /// The credential store, cswap's `self._store`.
    store: CredentialStore,
    /// The usage store, cswap's `self._usage_store`.
    usage_store: UsageStore,
    sequence_file: PathBuf,
    configs_dir: PathBuf,
    credentials_dir: PathBuf,
    lock_file: PathBuf,
    /// `_probe_verdicts`: definitive ownership verdicts for credential lineages, keyed by
    /// [`Switcher::lineage_key`] (the slot's full identity, so a slot re-created for another
    /// account never inherits its predecessor's). `true` when the profile oracle resolved the
    /// lineage to the slot's identity (or Koyo produced it with a refresh), `false` when it
    /// resolved to a foreign one. Probe failures and unverifiable answers are never cached: a
    /// `false` on partial evidence would block a legitimate resync for the process's life. The
    /// locked paths consult it because network under locks is forbidden.
    probe_verdicts: Mutex<HashMap<LineageKey, bool>>,
    /// The wait for Claude Code's locks: `None` is their default (`claude_locks`); tests shorten
    /// it as cswap's patch `DEFAULT_TIMEOUT_S`.
    claude_lock_timeout: Option<Duration>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Switcher {
    /// cswap's `ClaudeAccountSwitcher.__init__` on `env`'s store: the store's paths, a
    /// credential store, a usage store under `cache/`. Writes nothing: the folders are made by
    /// the operations that need them, as cswap's `_setup_directories`.
    pub fn new(env: Env) -> Switcher {
        Switcher {
            store: CredentialStore::new(env.clone()),
            usage_store: UsageStore::new(&env),
            sequence_file: paths::store::sequence_file(&env),
            configs_dir: paths::store::configs_dir(&env),
            credentials_dir: paths::store::credentials_dir(&env),
            lock_file: paths::store::lock_file(&env),
            probe_verdicts: Mutex::new(HashMap::new()),
            claude_lock_timeout: None,
            env,
        }
    }

    /// A cached ownership verdict for a lineage key, if one was recorded.
    pub(crate) fn probe_verdict(&self, key: &LineageKey) -> Option<bool> {
        locked(&self.probe_verdicts).get(key).copied()
    }
}
