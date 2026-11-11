//! Port of claude-swap's `switcher.py` (0fe22a8), `_build_accounts_info`: each managed slot of
//! the roster (`roster.rs`) with its credential for one pass of usage collection or a
//! snapshot, read in one place. The active slot's credential is Claude Code's live one; every
//! other slot's is its backup, through the display read cache (nothing writes or spends these
//! bytes: the refresh gate re-reads under its lock).
//!
//! cswap records the active read's verdict per thread and its fetch pool hands it to each
//! worker (`_with_active_verdict`: measured 30 of 30 verdicts lost without it). Here the
//! verdict rides as a value on the active row ([`AccountInfo::active`]), and no other row has
//! one, as cswap's reset at the start of each build.
//!
//! Left out: the alias column, with aliases.

use crate::Result;
use crate::credentials::ActiveVerdict;
use crate::switcher::{RosterSlot, Switcher};

/// One row of cswap's accounts info, `(num, email, org_name, org_uuid, is_active, creds,
/// alias)`: the roster's slot, the active read's verdict on the active row, and the
/// credential. No `Debug`: it holds a credential.
#[derive(Clone)]
pub(crate) struct AccountInfo {
    pub slot: RosterSlot,
    /// The active read's verdict on the live login's row; `None` on every other.
    pub active: Option<ActiveVerdict>,
    /// The live credential on the active row, the backup on the others; `""` for none.
    pub creds: String,
}

impl AccountInfo {
    pub(crate) fn num(&self) -> &str {
        &self.slot.number
    }

    pub(crate) fn email(&self) -> &str {
        &self.slot.identity.email
    }

    pub(crate) fn org_uuid(&self) -> &str {
        &self.slot.identity.organization_uuid
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

impl Switcher {
    /// `_build_accounts_info` on a roster read now.
    ///
    /// # Errors
    /// As [`Switcher::roster`].
    pub(crate) fn build_accounts_info(&self) -> Result<Vec<AccountInfo>> {
        let roster = self.roster()?;
        Ok(self.accounts_info(roster.current.as_deref(), roster.slots))
    }

    /// `_build_accounts_info` on a roster already read: one row per slot, in its order, the
    /// slot `current` the active one.
    pub(crate) fn accounts_info(
        &self,
        current: Option<&str>,
        slots: Vec<RosterSlot>,
    ) -> Vec<AccountInfo> {
        slots
            .into_iter()
            .map(|slot| {
                if current == Some(slot.number.as_str()) {
                    let active = self.store.read_active_credentials();
                    return AccountInfo {
                        active: Some(active.verdict()),
                        creds: active.value.unwrap_or_default(),
                        slot,
                    };
                }
                let creds =
                    self.store
                        .read_account_credentials(&slot.number, &slot.identity.email, true);
                AccountInfo {
                    slot,
                    active: None,
                    creds,
                }
            })
            .collect()
    }
}
