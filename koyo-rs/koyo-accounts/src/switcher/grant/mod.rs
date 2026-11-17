//! Port of claude-swap's `switcher.py` (0fe22a8), the refresh gate: `consume_backup_grant`,
//! through which a backup's refresh token is spent.
//!
//! A refresh token is one-time-use, so the POST must spend the provably-freshest copy of the
//! slot's grant, never a caller's snapshot, which may be a superseded generation. One consume
//! per slot is in flight at a time: a per-slot consume lock is held across the locked re-read,
//! the POST and the compare-and-swap persist (`locked.rs`), and Koyo's `.lock` is held around
//! the re-read and around the persist but never across the network call. A consumed
//! generation's successor is never dropped: when it can't be persisted it is parked in the
//! unclaimed stash, and the next pass adopts it (`stash.rs`) before spending anything.
//!
//! cswap's `_fetch_active_usage` is a second site that spends a backup's grant under the same
//! consume lock; Koyo never refreshes to read usage (`docs/claude-accounts.md` 4), so here the
//! gate is the only one.
//!
//! The gate never fails out: every outcome is a [`RefreshOutcome`], its `error` `None` on
//! success, else the refresh's own kind (`invalid_grant`, `no_refresh_token`,
//! `invalid_client`, `transient`) or the gate's (`store-unmirrored`, `consume-busy`,
//! `stash-unreadable`). A lock file the gate can't make, which cswap raises, is `transient`.
//!
//! Left out: the session-profile precedence of `_consume_backup_grant_locked` (a dead `cswap
//! run` profile holding a newer lineage, resynced into the backup before the POST) with
//! session mode, and with it the roster read that only fed it; `_post_backup_write`, which
//! only invalidates session profiles; the switcher's `_read_account_credentials_ex`, which
//! only forwards to the credential store's; and `persist_backup_credentials` and
//! `list_unclaimed_credentials`, which nothing in the app or the engine calls (the credential
//! store lists the stash itself).

use crate::locking::FileLock;
use crate::oauth::{CONSUME_BUSY_ERROR, RefreshOutcome, STORE_UNMIRRORED_ERROR, TRANSIENT_ERROR};
use crate::switcher::Switcher;

mod locked;
mod stash;

impl Switcher {
    /// cswap's `consume_backup_grant`: refreshes slot `account_num`'s backup through the gate
    /// and returns the slot's now-current credential, ours or a racing writer's newer one.
    /// `snapshot` is the caller's copy, which the gate may supersede; every outcome carries the
    /// fingerprint of the generation actually sent (`consumed_fp`), which a strike binds to.
    /// The caller must not hold Koyo's `.lock`.
    pub fn consume_backup_grant(
        &self,
        account_num: &str,
        email: &str,
        snapshot: &str,
    ) -> RefreshOutcome {
        if self
            .env
            .vars
            .securestorage_config_dir
            .as_deref()
            .is_some_and(|dir| !dir.is_empty())
        {
            log::warn!(
                "CLAUDE_SECURESTORAGE_CONFIG_DIR is set; Koyo mirrors it when capturing a \
                 credential but not when consuming one, so refusing to consume account \
                 {account_num}'s refresh token (unset the variable or run from a normal shell)."
            );
            return RefreshOutcome::failed(STORE_UNMIRRORED_ERROR);
        }
        let path = self
            .credentials_dir
            .join(format!(".consume-{account_num}.lock"));
        let _consume_lock = match FileLock::acquire(&self.env, &path, self.lock_timeout) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                log::info!(
                    "Another consume is in flight for account {account_num}; deferring to the \
                     next pass."
                );
                return RefreshOutcome::failed(CONSUME_BUSY_ERROR);
            }
            Err(error) => {
                log::warn!("Could not take account {account_num}'s consume lock: {error}");
                return RefreshOutcome::failed(TRANSIENT_ERROR);
            }
        };
        self.consume_backup_grant_locked(account_num, email, snapshot)
    }
}
