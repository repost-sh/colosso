//! Port of claude-swap's `switcher.py` (0fe22a8), `_resync_rotated_backup` whole, with the
//! ownership check it rests on, `_resolved_matches_slot_identity`.
//!
//! The active fetch serves usage off a credential the server just accepted. When its lineage
//! differs from the slot's backup, Claude Code rotated during normal use and nothing resynced
//! the backup: it holds the consumed predecessor, and the next spend of that grant answers
//! `invalid_grant` on a healthy slot. So the rotated credential is copied back, but only when
//! its lineage is provably the slot's: the config identity alone can't attribute it (a partial
//! cross-machine sync, a poll inside `/login`'s non-atomic write), and writing a foreign one
//! destroys the slot's only refresh token. The profile oracle is asked before any lock (no
//! network under them) while the access token is known fresh; definitive verdicts are kept per
//! lineage, failures and unverifiable answers never. Under Koyo's `.lock` and Claude Code's
//! credential locks the identity, the verdict and the live lineage are checked again before the
//! write. Best-effort: any failure leaves the backup as it was.
//!
//! The once-only provenance warning (`_provenance_warned`) and `_live_identity_matches`, which
//! only the resync uses, are here too. Left out: `_post_backup_write` after the write (it only
//! invalidates session profiles).

use crate::claude_locks::claude_credentials_lock;
use crate::locked;
use crate::locking::FileLock;
use crate::oauth::{
    AccountIdentity, extract_access_token, extract_oauth_data, fetch_oauth_profile,
    has_full_token_pair,
};
use crate::switcher::Switcher;
use crate::switcher::identity::same_lineage;
use crate::switcher::info::AccountInfo;
use crate::{AccountsError, Result};

/// A full token pair in `creds`. A Claude Code wipe empties the fields in place, and an
/// access-only blob would replace the backup's only refresh token.
fn full_pair(creds: &str) -> bool {
    extract_oauth_data(creds).is_some_and(|oauth| has_full_token_pair(&oauth))
}

impl Switcher {
    /// `_resync_rotated_backup`: copies the active row's live credential, just accepted by the
    /// server, into the slot's backup when it is a newer generation of the slot's own lineage.
    /// Never fails: the next pass retries.
    pub(crate) fn resync_rotated_backup(&self, info: &AccountInfo) {
        let account_num = info.num();
        match self.try_resync(account_num, info.email(), info.org_uuid(), &info.creds) {
            Ok(()) => {}
            // A holder is mid-operation; the next pass retries.
            Err(error) if error.is_lock() => {}
            Err(error) => log::warn!(
                "Backup resync for account {account_num} failed; the next expiry may refuse to \
                 refresh until a switch resyncs it: {error}"
            ),
        }
    }

    fn try_resync(
        &self,
        account_num: &str,
        email: &str,
        org_uuid: &str,
        creds: &str,
    ) -> Result<()> {
        if !full_pair(creds) {
            return Ok(()); // never seed a backup with a partial pair
        }
        let backup = self
            .store
            .read_account_credentials(account_num, email, false);
        if !backup.is_empty() && same_lineage(creds, &backup) {
            return Ok(()); // same lineage: nothing drifted
        }
        let verdict = self.lineage_verdict(account_num, email, creds)?;
        if verdict == Some(false) {
            return Ok(()); // known foreign, already warned
        }
        if verdict.is_none() {
            let token = extract_access_token(creds).unwrap_or_default();
            let Some(resolved) = fetch_oauth_profile(&self.env, &token) else {
                log::debug!(
                    "Ownership probe for account {account_num}'s drifted live credential \
                     failed; resync skipped this pass."
                );
                return Ok(());
            };
            let Some(matched) = self.resolved_matches_slot_identity(account_num, &resolved)? else {
                log::debug!(
                    "Ownership of account {account_num}'s drifted live credential is \
                     unverifiable (no stored uuid, partial profile); resync skipped this pass."
                );
                return Ok(());
            };
            // Keyed AFTER the match: an email-path affirmation just backfilled the slot's
            // uuid, and the verdict must live under the identity later consults rebuild.
            self.record_lineage_verdict(account_num, email, creds, matched)?;
            if !matched {
                if self.first_provenance_warning(account_num, email) {
                    log::warn!(
                        "Live credential resolves to a different account than \
                         Account-{account_num}'s identity; backup left untouched (foreign \
                         credential under a stale config)."
                    );
                }
                return Ok(());
            }
            self.clear_provenance_warning(account_num, email);
        }
        let _lock = FileLock::hold(&self.env, &self.lock_file, self.lock_timeout)?;
        let _credentials_lock = claude_credentials_lock(&self.env, self.claude_lock_timeout)?;
        // A switch or `/login` in the gap: the live store is no longer this account's.
        if !self.live_identity_matches(email, org_uuid) {
            return Ok(());
        }
        // Slot mutations hold this lock, so rebuilding the key revalidates that the slot still
        // IS the account the oracle affirmed.
        if self.lineage_verdict(account_num, email, creds)? != Some(true) {
            return Ok(());
        }
        // The live bytes must still carry the probed lineage with a full pair; the fingerprint
        // covers the refresh token, so the access token may have moved since the probe.
        let Some(live) = self
            .store
            .read_credentials()
            .filter(|live| !live.is_empty())
        else {
            return Ok(());
        };
        if !full_pair(&live) || !same_lineage(&live, creds) {
            return Ok(());
        }
        self.store
            .write_account_credentials(account_num, email, &live)?;
        log::info!(
            "Resynced account {account_num}'s backup to the rotated live credential (rotation \
             completed outside a collect pass)."
        );
        Ok(())
    }

    /// Whether `_provenance_warned` was empty of `(slot, email)`, which it now holds: warn when
    /// `true`.
    fn first_provenance_warning(&self, slot: &str, email: &str) -> bool {
        locked(&self.provenance_warned).insert((slot.to_owned(), email.to_owned()))
    }

    /// Drops a `_provenance_warned` entry once the lineage was affirmed, so a recurrence warns.
    fn clear_provenance_warning(&self, slot: &str, email: &str) {
        locked(&self.provenance_warned).remove(&(slot.to_owned(), email.to_owned()));
    }

    /// `_live_identity_matches`: whether the live config's identity is `(email, org_uuid)`
    /// right now, the under-lock re-check a switch or `/login` landing in the gap fails. The
    /// organization counts: two slots may share an email across organizations.
    fn live_identity_matches(&self, email: &str, org_uuid: &str) -> bool {
        self.get_current_account()
            .is_some_and(|(live_email, live_org)| live_email == email && live_org == org_uuid)
    }

    /// `_resolved_matches_slot_identity`: whether an oracle-resolved identity is the slot's
    /// account, uuid first (an email can be recycled). `Some(true)` the same account, recording
    /// the resolved uuid on a slot that has none; `Some(false)` provably another (safe to
    /// keep); `None` unverifiable (a uuid-less slot and an identity too partial to affirm or
    /// condemn), never kept.
    ///
    /// # Errors
    /// As [`Switcher::account_identity`] and [`Switcher::backfill_account_uuid`].
    pub(crate) fn resolved_matches_slot_identity(
        &self,
        account_num: &str,
        resolved: &AccountIdentity,
    ) -> Result<Option<bool>, AccountsError> {
        let own = self.account_identity(account_num)?;
        let r_uuid = resolved.uuid.trim();
        let r_email = resolved.email.as_deref().filter(|email| !email.is_empty());
        let r_org = resolved.organization_uuid.as_deref();
        if !r_uuid.is_empty() && !own.uuid.is_empty() {
            // The uuid is globally unique; the organization only corroborates, so a missing
            // one on either side is tolerated.
            return Ok(Some(
                r_uuid == own.uuid
                    && (r_org.is_none_or(str::is_empty)
                        || own.organization_uuid.is_empty()
                        || r_org == Some(own.organization_uuid.as_str())),
            ));
        }
        // A slot without a uuid: an email alone can't affirm (the same email exists across
        // personal and organization accounts), so only an exact (email, organization) match
        // with the organization present affirms, and records the uuid.
        if let Some(r_email) = r_email.filter(|email| *email == own.email) {
            let Some(r_org) = r_org else {
                return Ok(None);
            };
            if r_org == own.organization_uuid {
                self.backfill_account_uuid(account_num, r_uuid, Some(r_email), Some(r_org))?;
                return Ok(Some(true));
            }
            return Ok(Some(false));
        }
        if r_email.is_some() && r_org.is_some() {
            return Ok(Some(false));
        }
        Ok(None)
    }
}
