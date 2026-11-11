//! Port of claude-swap's `switcher.py` (0fe22a8), what a switch does with a live credential it
//! may not write into the outgoing slot: `_stash_live_credential`, the safety copy that
//! licenses overwriting the live store, and `_rescue_foreign_successor`, which advances the
//! owning slot's backup when the displaced credential is its newer generation (#489).
//!
//! The stash raises on failure: the bytes may be the only live copy of some account's refresh
//! token, so no copy, no switch. Its logged evidence (the credentials file's mtime, the live
//! `oauthAccount`, the resolved identity) is what identifies the writer, cswap's #117 being
//! unidentified.
//!
//! The rescue writes only on proof of a strictly newer generation: `expiresAt` moves forward on
//! every refresh and every login, so a live access token issued after the stored one means the
//! stored refresh token is the consumed predecessor, and keeping it strands the slot. Equal,
//! older, unreadable or a partial token pair leaves the backup as it was, and so does a stored
//! login with no expiry to compare (a setup token). The safety copy is already written, so the
//! rescue never removes the only copy of anything. cswap returns `false` for its store's and
//! lock's faults; Koyo's store has no file to fall back to, so a Keychain write the store
//! refuses is such a fault here too, and the guard's refusal still propagates.

use serde_json::{Value, json};

use crate::oauth::value::{number, truthy, truthy_at};
use crate::oauth::{AccountIdentity, credential_fingerprint, extract_oauth_data, has_login_token};
use crate::switcher::Switcher;
use crate::switcher::store::{Data, record, text};
use crate::switcher::switch::ForeignSlot;
use crate::{AccountsError, Result, paths};

/// `x or 0` of an `expiresAt`, as a number that isn't a bool: `None` for one `float()` can't
/// order on.
fn expiry_or_zero(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(value) if truthy(value) => number(value),
        _ => Some(0.),
    }
}

impl Switcher {
    /// `_stash_live_credential`: preserves an unowned live credential before it is
    /// overwritten, returning the safety copy's id. `live_config` is the live config as the
    /// switch's snapshot parsed it (`None` when absent or unreadable), for its `oauthAccount`.
    ///
    /// # Errors
    /// The stash's own: the caller must not overwrite the live store without the copy.
    pub(crate) fn stash_live_credential(
        &self,
        original_creds: &str,
        reason: &str,
        current_account: &str,
        resolved: Option<&AccountIdentity>,
        live_config: Option<&Data>,
    ) -> Result<String> {
        let creds_mtime = std::fs::metadata(paths::credentials_path(&self.env))
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|since| crate::oauth::iso_utc(since.as_secs_f64()));
        let mut context = json!({
            "reason": reason,
            "configSlot": current_account,
            "fingerprint": credential_fingerprint(original_creds),
            "liveOauthAccount": live_config.and_then(|config| config.get("oauthAccount")),
            "resolvedIdentity": resolved,
            "credentialsMtime": creds_mtime,
        });
        let context = context
            .as_object_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        let entry_id = self
            .store
            .write_unclaimed_credential(original_creds, context)?;
        log::warn!(
            "Live credential does not belong to Account-{current_account} ({reason}): stashed as \
             {entry_id} (credentials mtime {}). Something outside Koyo rewrote the live login \
             after the last switch.",
            creds_mtime.as_deref().unwrap_or("unknown")
        );
        Ok(entry_id)
    }

    /// `_rescue_foreign_successor`: advances `foreign`'s backup to `original_creds` when they
    /// are its strictly newer full token pair; whether it wrote. The backup compared is the one
    /// the attribution read, under the same locks, rather than a second Keychain read.
    ///
    /// # Errors
    /// A refusal from the write other than a store or lock fault (the guard's).
    pub(crate) fn rescue_foreign_successor(
        &self,
        foreign: &ForeignSlot,
        data: &Data,
        original_creds: &str,
    ) -> Result<bool> {
        let live = extract_oauth_data(original_creds).unwrap_or_default();
        if !(truthy_at(live.get("accessToken")) && truthy_at(live.get("refreshToken"))) {
            return Ok(false);
        }
        if foreign.unreadable {
            return Ok(false);
        }
        let foreign_slot = foreign.num.as_str();
        let email = text(record(data, foreign_slot), "email");
        let stored = extract_oauth_data(&foreign.backup).unwrap_or_default();
        let stored_exp = stored.get("expiresAt");
        if has_login_token(&stored) && stored_exp.and_then(number).is_none() {
            // A stored login with no expiry to compare can't be shown to be the older
            // generation, so it stays. A backup holding no login has nothing to lose.
            return Ok(false);
        }
        let (Some(live_exp), Some(stored_exp)) = (
            expiry_or_zero(live.get("expiresAt")),
            expiry_or_zero(stored_exp),
        ) else {
            // An expiry `float()` can't read is no ordering to write on.
            return Ok(false);
        };
        if live_exp <= stored_exp {
            return Ok(false);
        }
        match self
            .store
            .write_account_credentials(foreign_slot, email, original_creds)
        {
            Ok(()) => {
                log::info!(
                    "Resynced account {foreign_slot}'s backup to its newer live credential \
                     (found under another account's config label; the safety copy is kept)."
                );
                Ok(true)
            }
            Err(
                error @ (AccountsError::CredentialWrite(_)
                | AccountsError::Io(..)
                | AccountsError::Lock(_)),
            ) => {
                log::warn!(
                    "Could not advance account {foreign_slot}'s backup to its newer live \
                     credential; the safety copy keeps it: {error}"
                );
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}
