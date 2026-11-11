//! Port of claude-swap's `switcher.py` (0fe22a8), its policy half as far as the `disabled` flag
//! goes: a slot held out of automatic selection (`cswap disable`). A disabled slot stays
//! managed and stays a valid explicit `switch_to` target; auto-switch skips it. Re-enabling
//! returns it to its place in the sequence.
//!
//! The flag is stored as `"disabled": true` on the slot's record and popped, never written
//! `false`, when cleared, so a roster that never held a slot out is byte-identical to one from
//! before the flag existed.
//!
//! Left out: per-account thresholds and priority tiers (`set_account_threshold`,
//! `set_account_priority`, `normalize_account_*`), and with them `AccountPolicy`,
//! `_policy_from_data` and `account_policies`, which carry them beside the flag: the flag is
//! read with `_disabled_from_data`. The alias resolution too.

use serde_json::Value;

use crate::oauth::value::truthy_at;
use crate::switcher::Switcher;
use crate::switcher::store::{Data, is_active, record, record_mut, sequence};
use crate::{AccountsError, Result};

/// What [`Switcher::set_account_disabled`] did, for the app to say what cswap prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoldOutcome {
    pub account_num: String,
    pub email: String,
    /// `false` when the slot already was as asked ("already disabled" / "already enabled").
    pub changed: bool,
    /// Disabling the active account: it stays live until the person switches away.
    pub was_active: bool,
    /// After a disable, no slot is left for auto-switch to pick.
    pub rotation_empty: bool,
}

impl Switcher {
    /// `_disabled_from_data`: whether a slot is held out, in already-loaded data.
    pub(crate) fn disabled_from_data(data: &Data, account_num: &str) -> bool {
        record(data, account_num).is_some_and(|record| truthy_at(record.get("disabled")))
    }

    /// The switchable slots of loaded `data`, lazily: the flag, in memory, is checked before
    /// the Keychain-reading [`Switcher::account_is_switchable`].
    fn switchable_in<'a>(&'a self, data: &'a Data) -> impl Iterator<Item = Result<String>> + 'a {
        sequence(data).into_iter().filter_map(move |num| {
            if Switcher::disabled_from_data(data, &num) {
                return None;
            }
            match self.account_is_switchable(data, &num) {
                Ok(true) => Some(Ok(num)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            }
        })
    }

    /// `set_account_disabled`: holds a slot out of automatic selection, or returns it.
    ///
    /// # Errors
    /// [`AccountsError::Config`] when no account is managed yet or the email is ambiguous,
    /// [`AccountsError::AccountNotFound`] for an identifier that names none; as the write.
    pub fn set_account_disabled(&self, identifier: &str, disabled: bool) -> Result<HoldOutcome> {
        if !self.sequence_file.exists() {
            return Err(AccountsError::Config("No accounts are managed yet".into()));
        }
        let (account_num, email) = self.resolve_account(identifier)?;
        let mut data = self.sequence_data_or_empty()?;
        let Some(account) =
            record_mut(&mut data, &account_num).filter(|account| !account.is_empty())
        else {
            return Err(AccountsError::AccountNotFound(format!(
                "Account-{account_num} does not exist"
            )));
        };
        let changed = truthy_at(account.get("disabled")) != disabled;
        if changed {
            if disabled {
                account.insert("disabled".into(), Value::Bool(true));
            } else {
                account.shift_remove("disabled");
            }
            self.save_sequence(&mut data)?;
            let verb = if disabled { "Disabled" } else { "Enabled" };
            log::info!("{verb} account {account_num}: {email}");
        }
        let held = changed && disabled;
        let was_active = held && is_active(&data, &account_num);
        let rotation_empty = held && self.switchable_in(&data).next().transpose()?.is_none();
        Ok(HoldOutcome {
            account_num,
            email,
            changed,
            was_active,
            rotation_empty,
        })
    }
}
