//! The auto-switch engine's [`EngineHost`] on the switcher: cswap's engine (`autoswitch.py`,
//! 0fe22a8) holds the `ClaudeAccountSwitcher` and calls its methods; here each trait method is
//! that one call, and the switcher keeps every rule of its own (the store's lock, the
//! expected-identity guard of the backfill, the refresh gate, the switch's rollback). Nothing of
//! the engine's policy lives on this side.

use std::collections::{BTreeMap, BTreeSet};

use crate::Result;
use crate::autoswitch::EngineHost;
use crate::oauth::RefreshOutcome;
use crate::usage_store::UsageEntry;

use super::{Fetch, Roster, SlotIdentity, SwitchResult, Switcher};

impl EngineHost for Switcher {
    /// cswap's `usage_entries_by_account(fetch=..., scheduled=...)`: one collection pass that
    /// may fetch only the slots in `fetch` (none when it's empty, reading the store only).
    fn usage_entries_by_account(
        &self,
        fetch: &BTreeSet<String>,
        scheduled: bool,
    ) -> Result<BTreeMap<String, UsageEntry>> {
        Switcher::usage_entries_by_account(
            self,
            Fetch::Only {
                only: fetch,
                scheduled,
            },
        )
    }

    /// cswap's `switchable_account_numbers`, `disabled_account_numbers`, `account_email`,
    /// `account_identity`, `current_account_number` and `has_live_login`, from one read.
    fn roster(&self) -> Result<Roster> {
        Switcher::roster(self)
    }

    /// cswap's `backfill_account_uuid(number, uuid, expected_email=..., expected_org=...)`,
    /// the slot's identity as the engine read it the expected one.
    fn backfill_account_uuid(&self, number: &str, slot: &SlotIdentity, uuid: &str) -> Result<()> {
        Switcher::backfill_account_uuid(
            self,
            number,
            uuid,
            Some(&slot.email),
            Some(&slot.organization_uuid),
        )
    }

    /// cswap's `set_poll_policy_inputs(threshold, None, [])`: no weekly line, no models.
    fn set_poll_threshold(&self, threshold: f64) {
        self.set_poll_policy_inputs(threshold, None, &[]);
    }

    fn clear_poll_threshold(&self) {
        self.clear_poll_policy_inputs();
    }

    /// cswap's `read_account_credentials`: the backup, read past the display cache as cswap's
    /// `_read_account_credentials` defaults to; `""` when missing or unreadable.
    fn read_account_credentials(&self, number: &str, email: &str) -> String {
        self.store.read_account_credentials(number, email, false)
    }

    /// cswap's `consume_backup_grant`, the refresh gate.
    fn consume_backup_grant(&self, number: &str, email: &str, snapshot: &str) -> RefreshOutcome {
        Switcher::consume_backup_grant(self, number, email, snapshot)
    }

    /// cswap's `switch_to(number, json_output=True)`.
    fn switch_to(&self, number: &str) -> Result<SwitchResult> {
        Switcher::switch_to(self, number)
    }
}
