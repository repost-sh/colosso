//! Port of claude-swap's `switcher.py` (0fe22a8), the active account's usage fetch,
//! `_fetch_active_usage`, as far as it reads usage.
//!
//! An unexpired token is asked once, as cswap's fast path. When the server accepted it, the
//! slot's backup is resynced to it if Claude Code rotated during normal use
//! (`_resync_rotated_backup`, `resync.rs`), never from a degraded read, whose bytes may be the
//! consumed predecessor; and a lineage the profile oracle proved another account's earns the
//! `foreign credential` sentinel, since its quota isn't this slot's (#117's mis-keying shape).
//!
//! Where Koyo differs (`docs/claude-accounts.md` 4): cswap refreshes an expired token, or one
//! the server rejected with a 401, under Claude Code's own lock protocol and may spend the
//! slot's backup grant. Koyo never refreshes to read usage; Claude Code renews its own token on
//! its next call. So an expired token is cswap's deferral, the `token expired` sentinel
//! record (told by the fetch itself, which sends nothing for one), and a 401 its failure
//! record, which the store paces with its backoff. cswap's guard for a degraded read before
//! that refresh is kept for its label: the bytes may be a superseded generation, so `keychain
//! unavailable`, not `token expired`. Its refusal under `CLAUDE_SECURESTORAGE_CONFIG_DIR`
//! (`store-unmirrored`) is left out: it only refuses a refresh Koyo never makes. Left out with
//! the refresh: the provenance-unattributable warning, the locked re-read, the adoption of a
//! rotation Claude Code made, the backup restore, and the header probe.

use crate::credentials::ActiveVerdict;
use crate::oauth::{
    NO_ACCESS_TOKEN_ERROR, TOKEN_EXPIRED_ERROR, USAGE_FOREIGN_CREDENTIAL,
    USAGE_KEYCHAIN_UNAVAILABLE, USAGE_NO_CREDENTIALS, USAGE_TOKEN_EXPIRED,
    try_fetch_usage_for_account,
};
use crate::switcher::info::AccountInfo;
use crate::switcher::{Switcher, locked};
use crate::usage_store::FetchRecord;

impl Switcher {
    /// `_fetch_active_usage`: the active slot's reading from its live credential, `verdict`
    /// the active read's.
    pub(crate) fn fetch_active_usage(
        &self,
        info: &AccountInfo,
        verdict: ActiveVerdict,
    ) -> FetchRecord {
        let outcome = try_fetch_usage_for_account(&self.env, info.num(), &info.creds);
        match outcome.error.as_deref() {
            Some(NO_ACCESS_TOKEN_ERROR) => return FetchRecord::sentinel(USAGE_NO_CREDENTIALS),
            // cswap's `_defer`, with no refresh (the module says why).
            Some(TOKEN_EXPIRED_ERROR) if verdict.degraded => {
                return FetchRecord::sentinel(USAGE_KEYCHAIN_UNAVAILABLE);
            }
            Some(TOKEN_EXPIRED_ERROR) => return FetchRecord::sentinel(USAGE_TOKEN_EXPIRED),
            _ => {}
        }
        if outcome.usage.is_some() {
            if !verdict.degraded {
                self.resync_rotated_backup(info);
            }
            if self.known_foreign(info) {
                // The probe proved the served credential another account's: its quota isn't
                // this slot's. The sentinel reads as unknown headroom, and auto-switch's
                // failover is what repairs the drift.
                return FetchRecord::sentinel(USAGE_FOREIGN_CREDENTIAL);
            }
        }
        // A locally valid token the server rejected keeps its 401 as the record: the store
        // paces it with backoff instead of "token expired" mislabeling an unexpired token.
        FetchRecord::from(outcome)
    }

    /// Whether the profile oracle proved the row's live lineage another account's than the
    /// slot's (a cached `false` verdict). A roster that can't be read is no verdict.
    fn known_foreign(&self, info: &AccountInfo) -> bool {
        // No verdict at all, the common case: no roster read to build a key.
        if locked(&self.probe_verdicts).is_empty() {
            return false;
        }
        self.lineage_verdict(info.num(), info.email(), &info.creds)
            .is_ok_and(|verdict| verdict == Some(false))
    }
}
