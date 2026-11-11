//! What the auto-switch engine asks of the switcher: cswap's engine (`autoswitch.py`, 0fe22a8)
//! takes the `ClaudeAccountSwitcher` and calls its methods; here they're a trait the switcher
//! implements (`switcher/engine_host.rs`), so the engine is built and tested apart from it (on a
//! fake in its tests).
//!
//! Each method says what cswap's switcher does for it, which is what an implementation owes.
//! cswap's six reads of the slots and the Mac's login (`switchable_account_numbers`,
//! `disabled_account_numbers`, `account_email`, `account_identity`, `current_account_number`,
//! `has_live_login`) are one [`EngineHost::roster`] here, read once per tick, and again only
//! after a switch, for the wait. Of cswap's other methods, four aren't asked for:
//! `account_kind_for` (Koyo manages no API-key account, so every account is OAuth),
//! `live_session_pids_for` (cswap skipped a target a live `cswap run` session owned, so one
//! rotating refresh token never sat in two config folders; Koyo has no session mode),
//! `account_policies` (it carried per-account thresholds and priority tiers, both left out: one
//! global threshold) and `backup_dir` (the state file is at `paths::store::autoswitch_state`).

use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::oauth::RefreshOutcome;
use crate::switcher::{Roster, SlotIdentity, SwitchResult};
use crate::usage_store::UsageEntry;

/// What the engine asks of the switcher.
pub trait EngineHost: Send + Sync {
    /// cswap's `usage_entries_by_account(fetch, scheduled=...)`: the store-backed usage entry
    /// of every managed account, in sequence order, after one collection pass that may fetch
    /// only the slots in `fetch` (an empty set fetches nothing and reads the store). The
    /// collector still applies the store's rules to those (`UsageStore::reserve`: backoff,
    /// holds, leases, due-ness) and lays each pass's sentinels over the entries.
    /// `scheduled` preserves a valid future poll plan while letting a due plan beat the serve
    /// TTL; the engine passes `false` only to override a candidate-style plan on the active.
    fn usage_entries_by_account(
        &self,
        fetch: &BTreeSet<String>,
        scheduled: bool,
    ) -> Result<BTreeMap<String, UsageEntry>>;

    /// The managed slots and the Mac's login, read together (the module says which cswap
    /// reads this answers). A failure is the tick's error.
    fn roster(&self) -> Result<Roster>;

    /// cswap's `backfill_account_uuid`: records `uuid` on a slot whose uuid is empty, under
    /// the store's lock; an existing uuid is identity and is never rewritten. `slot` is the
    /// identity the engine read the slot with, so a slot removed and re-added since (another
    /// email or organization) isn't given its predecessor's uuid.
    fn backfill_account_uuid(&self, number: &str, slot: &SlotIdentity, uuid: &str) -> Result<()>;

    /// cswap's `set_poll_policy_inputs`: pins the line the collector's poll plans key on to the
    /// engine's (cswap stores it on the switcher, a bare attribute write). The engine has no
    /// weekly line and no models, so its threshold is all it pins.
    fn set_poll_threshold(&self, threshold: f64);

    /// cswap's `read_account_credentials`: the slot's stored backup credential JSON, empty
    /// when missing or unreadable. Never logged.
    fn read_account_credentials(&self, number: &str, email: &str) -> String;

    /// cswap's `consume_backup_grant`, the refresh gate: under the slot's consume lock,
    /// re-reads the freshest copy of the slot's grant (adopting a stashed successor), POSTs it
    /// with no other lock held, then persists the successor by a fingerprint compare-and-swap
    /// or stashes it. `snapshot` is the caller's copy, which the gate may supersede. The
    /// outcome's `error` is `None` on success, else `invalid_grant`, `no_refresh_token`,
    /// `invalid_client`, `transient` or one of the gate's own (`store-unmirrored`,
    /// `consume-busy`, `stash-unreadable`); `credentials` is the slot's credential after a
    /// success, `consumed_fp` the fingerprint of the generation sent, and `token_account` the
    /// identity the token endpoint sent, if any.
    fn consume_backup_grant(&self, number: &str, email: &str, snapshot: &str) -> RefreshOutcome;

    /// cswap's `switch_to(number, json_output=True)`: makes the slot the Mac's login (backing
    /// up the outgoing one, rolling back on failure) and replans the new active row's poll. A
    /// target already active is a no-op, `switched: false` with its reason.
    fn switch_to(&self, number: &str) -> Result<SwitchResult>;
}
