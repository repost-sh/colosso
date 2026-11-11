//! Port of claude-swap's `switcher.py` (0fe22a8), the body of the refresh gate,
//! `_consume_backup_grant_locked` with its `stash_successor`: under the consume lock, adopt a
//! stashed successor and re-read the slot's backup under Koyo's `.lock`; POST outside it; then
//! compare-and-swap on the refresh-token fingerprint, and persist or stash.
//!
//! Before the POST nothing is spent, so every failure there is `transient` and costs a pass.
//! After it the grant is spent: the successor is written back only when the store still holds
//! the generation that was sent; a removed slot, an unreadable store, a racing writer's newer
//! lineage, a lock that stayed held or a failed write all park it in the stash instead
//! (`credentials::StashReason`), and when even the stash fails it survives only in the
//! returned outcome. A successor that leaves the slot on the spent generation demotes the
//! outcome to `transient` (with the credentials still riding along), since activating that
//! slot would install a token that can never refresh.

use crate::credentials::{StashMeta, StashReason};
use crate::locking::FileLock;
use crate::oauth::{
    REFRESH_TIMEOUT, RefreshOutcome, credential_fingerprint, extract_access_token,
    try_refresh_oauth_credentials,
};
use crate::subscription::usable_access_token;
use crate::switcher::Switcher;
use crate::{AccountsError, Result};

/// Why the gate stopped before the POST: the outcome's error kind.
type Deferred = &'static str;

/// Where a spent grant's successor went.
enum Persisted {
    /// Written back into the slot's backup.
    Written,
    /// Parked in the stash.
    Stashed(StashReason),
    /// Parked as a CAS conflict: a racing writer's newer lineage, `current`, is the slot's.
    Conflict { current: String },
    /// Neither written nor parked: it survives only in the outcome.
    Unpersisted,
}

impl Switcher {
    /// `_consume_backup_grant_locked`; the caller holds the slot's consume lock.
    pub(super) fn consume_backup_grant_locked(
        &self,
        account_num: &str,
        email: &str,
        snapshot: &str,
    ) -> RefreshOutcome {
        let refresh_input = match self.pre_consume(account_num, email) {
            Ok(refresh_input) => refresh_input,
            Err(kind) => return RefreshOutcome::failed(kind),
        };
        let consumed_fp = credential_fingerprint(&refresh_input);
        let input_access = usable_access_token(Some(&refresh_input), self.env.now());
        let snap_access = extract_access_token(snapshot).filter(|access| !access.is_empty());
        if let (Some(input_access), Some(snap_access)) = (input_access, snap_access)
            && input_access != snap_access
        {
            // The world moved past the caller's snapshot and the current generation is fresh:
            // the refresh the caller wanted happened (a racing gate's rotation, an adopted
            // stash). Spending another grant burns a generation for nothing. When the re-read
            // equals the snapshot (the 401-retry shape) this never fires.
            return RefreshOutcome {
                credentials: Some(refresh_input),
                consumed_fp,
                ..RefreshOutcome::default()
            };
        }

        let (successor, token_account) =
            match try_refresh_oauth_credentials(&self.env, &refresh_input, REFRESH_TIMEOUT) {
                RefreshOutcome {
                    credentials: Some(successor),
                    error: None,
                    token_account,
                    ..
                } if !successor.is_empty() => (successor, token_account),
                // Strikes follow the POSTed bytes, which may be a re-read, not the snapshot.
                failed => {
                    return RefreshOutcome {
                        consumed_fp,
                        ..failed
                    };
                }
            };

        let persisted =
            self.persist_successor(account_num, email, &successor, consumed_fp.as_deref());
        // A demoting outcome: the slot still holds the generation whose grant was just spent,
        // and callers read `error: None` as "freshened and safe to activate".
        let (credentials, demoted, stashed) = match persisted {
            Persisted::Written => (successor, false, false),
            Persisted::Conflict { current } => (current, false, false),
            Persisted::Stashed(reason) => (successor, reason.demotes(), reason.demotes()),
            Persisted::Unpersisted => (successor, true, false),
        };
        RefreshOutcome {
            credentials: Some(credentials),
            error: demoted.then(|| "transient".to_owned()),
            token_account,
            consumed_fp,
            stashed,
        }
    }

    /// The pre-consume window: under Koyo's `.lock`, the slot's freshest grant (a stashed
    /// successor adopted first), or the outcome to defer with. Nothing is spent here.
    fn pre_consume(&self, account_num: &str, email: &str) -> std::result::Result<String, Deferred> {
        let _lock = match FileLock::acquire(&self.env, &self.lock_file, self.lock_timeout) {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                // A holder (a switch, a collector) owns the slot; defer cleanly.
                log::info!(
                    "Slot lock held elsewhere; deferring account {account_num}'s backup refresh \
                     to the next pass."
                );
                return Err("transient");
            }
            Err(error) => return Err(pre_consume_failed(account_num, &error)),
        };
        let (mut current, unreadable) =
            self.store
                .read_account_credentials_ex(account_num, email, false);
        if unreadable {
            // The backup may exist but can't be seen (a locked Keychain): the snapshot is
            // exactly the possibly-superseded copy this gate exists never to spend.
            log::info!(
                "Backup for account {account_num} unreadable (keychain); deferring the refresh."
            );
            return Err("transient");
        }
        match self.adopt_stashed_successor(account_num, email, &current) {
            Ok(Some(adopted)) => current = adopted,
            Ok(None) => {}
            Err(AccountsError::CredentialRead(why)) => {
                // The slot's only successor is unreadable: the bytes are the sole copy of a
                // generation already spent, and nothing tells "locked for a minute" from
                // "locked forever", so defer, but say so rather than "transient", which reads
                // as network trouble on a condition only the person can clear.
                log::info!(
                    "Account {account_num}'s stashed successor is unreadable; deferring the \
                     refresh: {why}"
                );
                return Err("stash-unreadable");
            }
            Err(error) => return Err(pre_consume_failed(account_num, &error)),
        }
        if current.is_empty() {
            // Absent, not unreadable: the slot was removed since the caller read. The caller's
            // snapshot would spend a grant for an account the person just deleted and stash a
            // successor no slot can adopt.
            log::info!(
                "Account {account_num}'s stored credential is gone; deferring the refresh rather \
                 than consuming a grant for a slot that no longer exists."
            );
            return Err("transient");
        }
        Ok(current)
    }

    /// The persist after the POST: the compare-and-swap under Koyo's `.lock`, the stash when
    /// the lock stays held or the write fails. The grant is spent and the caller must not
    /// fail, so this never does.
    fn persist_successor(
        &self,
        account_num: &str,
        email: &str,
        successor: &str,
        consumed_fp: Option<&str>,
    ) -> Persisted {
        let failure = match self.cas_locked(account_num, email, successor, consumed_fp) {
            Ok(persisted) => return persisted,
            Err(error) if error.is_lock() => {
                // The token works; the next pass adopts it from the stash.
                let reason = StashReason::PersistLockFailed;
                match self.stash_successor(successor, account_num, consumed_fp, reason) {
                    Ok(()) => return Persisted::Stashed(reason),
                    Err(error) => error,
                }
            }
            Err(error) => error,
        };
        log::warn!(
            "Persisting account {account_num}'s refreshed credential failed; stashing instead: \
             {failure}"
        );
        let reason = StashReason::PersistFailed;
        match self.stash_successor(successor, account_num, consumed_fp, reason) {
            Ok(()) => Persisted::Stashed(reason),
            Err(error) => {
                // The stash is the last resort; when it fails too the successor survives only
                // in the outcome.
                log::error!(
                    "Account {account_num}'s consumed successor could not be persisted or \
                     stashed — it survives only for this pass. Fix the storage failure, then \
                     sign in again and add the account if it is quarantined: {error}"
                );
                Persisted::Unpersisted
            }
        }
    }

    /// The compare-and-swap on the refresh-token fingerprint, under Koyo's `.lock`.
    ///
    /// # Errors
    /// The lock that stayed held, a write or a stash that failed.
    fn cas_locked(
        &self,
        account_num: &str,
        email: &str,
        successor: &str,
        consumed_fp: Option<&str>,
    ) -> Result<Persisted> {
        let _lock = FileLock::hold(&self.env, &self.lock_file, self.lock_timeout)?;
        let (store_now, unreadable) =
            self.store
                .read_account_credentials_ex(account_num, email, false);
        let reason = if unreadable {
            // The Keychain locked during the POST: the CAS can't be evaluated, and the slot
            // may still hold the generation that was spent, so stash and demote.
            StashReason::StoreUnreadable
        } else if store_now.is_empty() {
            // Emptied mid-POST (a removal): writing back would resurrect a deleted account.
            StashReason::SlotRemoved
        } else if credential_fingerprint(&store_now).as_deref() != consumed_fp {
            // A writer replaced the lineage while the POST was in flight: stash ours, adopt
            // the store's newer one.
            self.stash_successor(
                successor,
                account_num,
                consumed_fp,
                StashReason::CasConflict,
            )?;
            return Ok(Persisted::Conflict { current: store_now });
        } else {
            self.store
                .write_account_credentials(account_num, email, successor)?;
            return Ok(Persisted::Written);
        };
        self.stash_successor(successor, account_num, consumed_fp, reason)?;
        Ok(Persisted::Stashed(reason))
    }

    /// cswap's `stash_successor`: parks a consumed generation's successor where the next gate
    /// pass adopts it; `consumedFp`, the generation it superseded, is the adoption key.
    ///
    /// # Errors
    /// The stash's own.
    fn stash_successor(
        &self,
        successor: &str,
        account_num: &str,
        consumed_fp: Option<&str>,
        reason: StashReason,
    ) -> Result<()> {
        let meta = StashMeta::successor(reason, account_num, consumed_fp, successor);
        self.store.stash_successor(successor, &meta)?;
        log::warn!("{}", reason.note(account_num));
        Ok(())
    }
}

/// cswap's generic pre-consume handler: nothing is spent yet, so defer. A write before this
/// point (an adoption) may already have advanced the slot; the next pass re-reads it fresh.
fn pre_consume_failed(account_num: &str, error: &AccountsError) -> Deferred {
    log::warn!("Pre-consume window failed for account {account_num}; deferring: {error}");
    "transient"
}
