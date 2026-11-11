//! Port of claude-swap's `credentials.py` (0fe22a8), its backup half: each account's backup is
//! a Keychain item under [`BACKUP_KEYCHAIN_SERVICE`], account `account-<n>-<email>`, with the
//! generation it replaced kept as `account-<n>-<email>.prev`.
//!
//! - A backup read honours the failure cooldown: inside it the read answers "unreadable"
//!   without spawning `security` (#496). The active read and the writes already stay off the
//!   Keychain then; backup reads were the one path that kept probing.
//! - A display read (`cached`) may be served from a per-slot cache for up to
//!   [`BACKUP_READ_CACHE_TTL_S`](super::BACKUP_READ_CACHE_TTL_S): a panel repainting every few
//!   seconds mustn't spawn `security` per slot per pass. Only successes are cached; the cache
//!   is dropped after every own write or delete of a backup, and whenever `sequence.json`
//!   changes (inode, mtime, size: any process's add, remove or switch). Anything whose bytes
//!   are spent, written or compared reads fresh.
//! - The previous generation is kept in the Keychain, never a file: keeping it must not weaken
//!   where the person's credentials live. It's best-effort; the switch's provenance check and
//!   the unclaimed stash are the safety boundary.
//!
//! Left out: the `.enc` backups, their `.enc`-wins reads and reconcile, `.enc.prev` copies and
//! the legacy file sweep. A backup write while the Keychain is backed off, or one the Keychain
//! refuses, fails rather than writing a file. `delete_account_credentials_strict` too: nothing
//! calls it.

use std::time::Instant;

use crate::credentials::{BackupReadError, CredentialStore, Generation, generation};
use crate::keychain::{self, KeychainError};
use crate::paths::{self, BACKUP_KEYCHAIN_SERVICE};
use crate::{AccountsError, Result};

/// A backup's Keychain account.
pub fn backup_username(account_num: &str, email: &str) -> String {
    format!("account-{account_num}-{email}")
}

/// The retained previous generation's Keychain account.
pub fn prev_backup_username(account_num: &str, email: &str) -> String {
    format!("{}.prev", backup_username(account_num, email))
}

impl CredentialStore {
    /// `sequence.json`'s generation, or `None` when it can't be read: the cheap "did the
    /// account set change?" signal.
    fn registry_generation(&self) -> Option<Generation> {
        generation(&paths::store::sequence_file(&self.env))
    }

    /// Forgets every cached backup read; the epoch's bump voids a fill that was mid-`security`
    /// when this landed, whose value may predate the change.
    pub fn drop_backup_read_cache(&self) {
        let mut state = self.state();
        state.backup_read_cache.clear();
        state.backup_read_cache_epoch += 1;
    }

    /// Reads a backup from the Keychain: `""` when the item is absent.
    ///
    /// # Errors
    /// [`BackupReadError::BackedOff`] inside the cooldown, without spawning `security`; the
    /// Keychain's failure otherwise. Several threads may pass the deadline together, so a
    /// recovery is a short burst rather than exactly one probe.
    pub fn kc_read_backup(
        &self,
        account_num: &str,
        email: &str,
        cached: bool,
    ) -> Result<String, BackupReadError> {
        let key = (account_num.to_owned(), email.to_owned());
        let now = Instant::now();
        let mut epoch = None;
        if cached {
            let generation = self.registry_generation();
            let mut state = self.state();
            if generation != state.backup_read_cache_gen {
                // The registry changed: an entry may describe a slot that moved or was re-added.
                state.backup_read_cache.clear();
                state.backup_read_cache_epoch += 1;
                state.backup_read_cache_gen = generation;
            }
            if let Some((value, read_at)) = state.backup_read_cache.get(&key)
                && now.saturating_duration_since(*read_at) < state.cache_ttl
            {
                return Ok(value.clone());
            }
            epoch = Some(state.backup_read_cache_epoch);
        }
        {
            let mut state = self.state();
            if let Some(deadline) = state.disabled_until
                && Instant::now() < deadline
            {
                if state.backoff_announced_until != Some(deadline) {
                    state.backoff_announced_until = Some(deadline);
                    log::info!(
                        "Keychain backed off after a failure: backup reads report unreadable \
                         for up to {:.0} s instead of probing",
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_secs_f64()
                    );
                }
                return Err(BackupReadError::BackedOff);
            }
        }
        let username = backup_username(account_num, email);
        let value = self
            .kc_call(|security| {
                keychain::get_password(security, BACKUP_KEYCHAIN_SERVICE, &username)
            })
            .map_err(BackupReadError::Keychain)?
            .unwrap_or_default();
        if let Some(epoch) = epoch {
            let mut state = self.state();
            // Stored only when nothing was dropped while `security` ran.
            if epoch == state.backup_read_cache_epoch {
                state.backup_read_cache.insert(key, (value.clone(), now));
            }
        }
        Ok(value)
    }

    /// Writes a backup to the Keychain, then drops the cache: after the write, never before
    /// ("drop, a cached miss reads the old item, the write lands" can't be seen by the epoch),
    /// and after a failed one too, which may still have changed the item.
    ///
    /// # Errors
    /// The Keychain's.
    pub fn kc_write_backup(
        &self,
        account_num: &str,
        email: &str,
        credentials: &str,
    ) -> Result<(), KeychainError> {
        let username = backup_username(account_num, email);
        let result = self.kc_call(|security| {
            keychain::set_password(security, BACKUP_KEYCHAIN_SERVICE, &username, credentials)
        });
        self.drop_backup_read_cache();
        result
    }

    /// Deletes a backup's Keychain item, then drops the cache, as [`Self::kc_write_backup`].
    ///
    /// # Errors
    /// The Keychain's; an absent item is deleted.
    pub fn kc_delete_backup(&self, account_num: &str, email: &str) -> Result<(), KeychainError> {
        let username = backup_username(account_num, email);
        let result = self.kc_call(|security| {
            keychain::delete_password(security, BACKUP_KEYCHAIN_SERVICE, &username)
        });
        self.drop_backup_read_cache();
        result
    }

    /// Deletes a backup's retained `.prev` item.
    ///
    /// # Errors
    /// The Keychain's; an absent item is deleted.
    pub fn kc_delete_backup_prev(
        &self,
        account_num: &str,
        email: &str,
    ) -> Result<(), KeychainError> {
        let username = prev_backup_username(account_num, email);
        self.kc_call(|security| {
            keychain::delete_password(security, BACKUP_KEYCHAIN_SERVICE, &username)
        })
    }

    /// Deletes a backup's Keychain item, a failure logged.
    pub fn delete_backup_keychain_quiet(&self, account_num: &str, email: &str) {
        if let Err(error) = self.kc_delete_backup(account_num, email) {
            log::warn!("Failed to delete credentials from Keychain: {error}");
        }
    }

    /// Reads a backup: `""` when there's none or it couldn't be read. `cached` lets a display
    /// or eligibility read be served from the cache.
    pub fn read_account_credentials(&self, account_num: &str, email: &str, cached: bool) -> String {
        self.read_account_credentials_ex(account_num, email, cached)
            .0
    }

    /// Reads a backup with its verdict: `(value, unreadable)`, `unreadable` when the read that
    /// gave `""` failed rather than found nothing, so the person is told to retry, not to add
    /// the account again. The verdict is this read's own, never a flag another read could
    /// overwrite: a sibling read on another thread used to flip it (2 of 60 measured), and the
    /// refresh gate then spent a superseded grant.
    pub fn read_account_credentials_ex(
        &self,
        account_num: &str,
        email: &str,
        cached: bool,
    ) -> (String, bool) {
        match self.kc_read_backup(account_num, email, cached) {
            Ok(value) => (value, false),
            // An expected skip, announced once when the cooldown began: unreadable, but not a
            // new failure to warn about on every lookup.
            Err(BackupReadError::BackedOff) => (String::new(), true),
            Err(BackupReadError::Keychain(error)) => {
                log::warn!("Failed to read credentials from Keychain: {error}");
                (String::new(), true)
            }
        }
    }

    /// Writes a backup to the Keychain, the generation it replaces kept first as `.prev`.
    /// Nothing else (the session invalidation is the switcher's), and it fails before
    /// returning, so the caller's follow-up runs only after a write that landed.
    ///
    /// # Errors
    /// [`AccountsError::CredentialWrite`] when the Keychain is backed off or refuses the write;
    /// cswap writes an `.enc` file then, Koyo writes nothing.
    pub fn write_account_credentials(
        &self,
        account_num: &str,
        email: &str,
        credentials: &str,
    ) -> Result<()> {
        self.retain_previous_backup(account_num, email, credentials);
        if !self.use_keychain() {
            log::warn!("Keychain backup write skipped: the Keychain is backed off after a failure");
            return Err(AccountsError::CredentialWrite(format!(
                "Failed to write account {account_num}'s credentials: the Keychain is backed \
                 off after a failure, and Koyo keeps no file copy to write instead. Retry in a \
                 minute."
            )));
        }
        if let Err(error) = self.kc_write_backup(account_num, email, credentials) {
            log::warn!("Keychain backup write failed: {error}");
            return Err(AccountsError::CredentialWrite(format!(
                "Failed to write account {account_num}'s credentials: {error}"
            )));
        }
        Ok(())
    }

    /// Deletes a backup and its `.prev`, the legacy `account-None-<email>` alias included,
    /// best-effort: an item a locked Keychain keeps is unreferenced cruft (the slot is gone
    /// from `sequence.json`, and a re-add overwrites it).
    pub fn delete_account_credentials(&self, account_num: &str, email: &str) {
        let mut nums = vec![account_num];
        if account_num != "None" {
            nums.push("None");
        }
        for num in nums {
            self.delete_backup_keychain_quiet(num, email);
            self.delete_previous_backup(num, email);
        }
    }

    /// Drops a slot's `.prev`, best-effort: with the backup, and on its own when a key's
    /// history stops belonging to its account, so a recovery never resurrects another
    /// account's generation.
    pub fn delete_previous_backup(&self, account_num: &str, email: &str) {
        if let Err(error) = self.kc_delete_backup_prev(account_num, email) {
            log::warn!("Failed to delete .prev from Keychain: {error}");
        }
    }

    /// Keeps the slot's current backup as `.prev` before `new_credentials` replaces it. When
    /// the current one can't be read, no `.prev` is written and the write goes ahead: a copy of
    /// the incoming bytes was tried in cswap and outranked a real `.prev` (round 10 withdrew
    /// it), so the honest answer is no recovery copy for this write.
    pub fn retain_previous_backup(&self, account_num: &str, email: &str, new_credentials: &str) {
        let (current, unreadable) = self.read_account_credentials_ex(account_num, email, false);
        if unreadable {
            // INFO: the read already warned (or the cooldown announced itself once).
            log::info!(
                "Could not retain previous credential generation for account {account_num}: the \
                 current backup exists but could not be read (not absent) — no .prev recovery \
                 copy will exist for this write"
            );
            return;
        }
        if current.is_empty() || current == new_credentials {
            return;
        }
        if !self.use_keychain() {
            log::warn!(
                "Failed to retain previous credential generation for account {account_num}: the \
                 Keychain is backed off, and Koyo keeps no file copy"
            );
            return;
        }
        let username = prev_backup_username(account_num, email);
        if let Err(error) = self.kc_call(|security| {
            keychain::set_password(security, BACKUP_KEYCHAIN_SERVICE, &username, &current)
        }) {
            log::warn!(
                "Failed to retain previous credential generation for account {account_num}: \
                 {error}"
            );
        }
    }

    /// Reads the retained previous generation: `""` when it's absent or can't be read. For the
    /// tests of [`Self::retain_previous_backup`]: nothing else reads a `.prev`.
    #[cfg(test)]
    pub(crate) fn read_previous_backup(&self, account_num: &str, email: &str) -> String {
        let username = prev_backup_username(account_num, email);
        match self.kc_call(|security| {
            keychain::get_password(security, BACKUP_KEYCHAIN_SERVICE, &username)
        }) {
            Ok(value) => value.unwrap_or_default(),
            Err(error) => {
                log::warn!("Failed to read .prev from Keychain: {error}");
                String::new()
            }
        }
    }
}
