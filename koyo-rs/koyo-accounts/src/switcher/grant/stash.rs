//! Port of claude-swap's `switcher.py` (0fe22a8), the gate's side of the unclaimed stash:
//! `_adopt_stashed_successor`, which completes a prior gate's failed persist.
//!
//! When the slot still stores exactly the generation a stashed successor superseded, its
//! refresh token is already spent and the stash holds the live successor: writing it back is
//! the pending persist, and saves spending a grant at all. The scan of the stash, which fails
//! closed and retires the rows no pass can adopt, is the credential store's
//! (`CredentialStore::stashed_successor`, beside the manifest); the write-back is here.

use crate::Result;
use crate::oauth::credential_fingerprint;
use crate::switcher::Switcher;

impl Switcher {
    /// `_adopt_stashed_successor`: the adopted credential when the slot's `current` generation
    /// has a stashed successor, written back into the backup; `None` when nothing applies.
    /// The caller holds Koyo's `.lock`.
    ///
    /// # Errors
    /// As [`crate::credentials::CredentialStore::stashed_successor`]; the backup write's.
    pub(crate) fn adopt_stashed_successor(
        &self,
        account_num: &str,
        email: &str,
        current: &str,
    ) -> Result<Option<String>> {
        let Some(consumed_fp) = credential_fingerprint(current) else {
            return Ok(None);
        };
        let Some(found) = self.store.stashed_successor(account_num, &consumed_fp)? else {
            return Ok(None);
        };
        self.store
            .write_account_credentials(account_num, email, &found.credentials)?;
        // The slot is advanced: a failed retire only leaves a row to retry.
        self.store.retire_stash_entry(&found.entry_id, account_num);
        log::info!(
            "Adopted account {account_num}'s stashed successor ({}): the stored generation was \
             already consumed by the gate pass that stashed it.",
            found.reason.map_or("unknown", |reason| reason.as_str())
        );
        Ok(Some(found.credentials))
    }
}
