//! Port of claude-swap's `switcher.py` (0fe22a8), `remove_account`: a slot's credential and
//! config backups removed and the slot dropped from `sequence.json`. Removing the active
//! account is allowed; the result says it was, as cswap warns. The live login is left as it is.
//!
//! Left out: the confirmation prompt (the app asks the person first), the interactive choice
//! between slots sharing an email (refused as ambiguous), aliases, the live-session check
//! (`_ensure_no_live_session`), `_refuse_session_shell`, the org-field migration and the
//! directory-mapping prune.

use serde_json::Value;

use crate::switcher::Switcher;
use crate::switcher::identity::{is_digits, validate_email};
use crate::switcher::store::{is_active, record, slot_int, text};
use crate::{AccountsError, Result};

/// The slot [`Switcher::remove_account`] removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedAccount {
    pub account_num: String,
    pub email: String,
    /// It was the recorded active account (cswap's "is currently active" warning).
    pub was_active: bool,
}

impl Switcher {
    /// `remove_account` for a slot number or an email.
    ///
    /// # Errors
    /// [`AccountsError::Config`] when no account is managed yet or the email is ambiguous,
    /// [`AccountsError::Validation`] for an identifier that is neither a number nor an email,
    /// [`AccountsError::AccountNotFound`]; the file system's.
    pub fn remove_account(&self, identifier: &str) -> Result<RemovedAccount> {
        if !self.sequence_file.exists() {
            return Err(AccountsError::Config("No accounts are managed yet".into()));
        }
        if !is_digits(identifier) && !validate_email(identifier) {
            return Err(AccountsError::Validation(format!(
                "Invalid account identifier: {identifier}"
            )));
        }
        let Some(account_num) = self.resolve_account_identifier(identifier)? else {
            return Err(AccountsError::AccountNotFound(format!(
                "No account found with identifier: {identifier}"
            )));
        };
        let mut data = self.sequence_data_or_empty()?;
        let Some(account_info) = record(&data, &account_num).filter(|info| !info.is_empty()) else {
            return Err(AccountsError::AccountNotFound(format!(
                "Account-{account_num} does not exist"
            )));
        };
        let email = text(Some(account_info), "email").to_owned();
        let was_active = is_active(&data, &account_num);
        if was_active {
            log::warn!("Account-{account_num} ({email}) is currently active");
        }

        self.delete_account_files(&account_num, &email)?;

        let number = slot_int(&account_num)?;
        if let Some(accounts) = data.get_mut("accounts").and_then(Value::as_object_mut) {
            accounts.shift_remove(&account_num);
        }
        let kept: Vec<Value> = data
            .get("sequence")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|num| num.as_u64() != Some(number))
            .cloned()
            .collect();
        data.insert("sequence".into(), Value::Array(kept));
        self.save_sequence(&mut data)?;
        log::info!("Removed account {account_num}: {email}");
        Ok(RemovedAccount {
            account_num,
            email,
            was_active,
        })
    }
}
