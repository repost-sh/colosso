//! Port of claude-swap's `switcher.py` (0fe22a8), its identity half: the live login's identity
//! from Claude Code's global config, the slot holding an identity, each slot's stored identity,
//! and the resolution of an identifier (a slot number or an email) to a slot.
//!
//! An identity is `(email, organizationUuid)`: one email can hold a personal account and an
//! organisation account, two slots. A uuid, where both sides carry one, decides ahead of the
//! email: uuids are stable where an email can be recycled.
//!
//! Left out: aliases in the resolution (`_find_account_by_alias`), and the interactive choice
//! between slots sharing an email: an ambiguous email is refused, as cswap's JSON mode does.

use serde_json::Value;

use crate::locking::FileLock;
use crate::oauth::credential_fingerprint;
use crate::switcher::store::{Data, accounts, record, record_mut, text};
use crate::switcher::{LineageKey, Switcher};
use crate::{AccountsError, Result};

/// An identity as cswap passes it around: a slot's stored one (its `account_identity` dict),
/// or the live one (`_get_current_identity_triple`, `uuid` being `accountUuid`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SlotIdentity {
    pub email: String,
    pub organization_uuid: String,
    pub uuid: String,
}

/// An organisation as cswap names it in a message: its uuid or name, `personal` for none.
pub(crate) fn org_label(org: &str) -> &str {
    if org.is_empty() { "personal" } else { org }
}

/// `_validate_email`: `^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$`, matched as Python's
/// `re.match` does (`$` also matches before one trailing newline).
pub(crate) fn validate_email(email: &str) -> bool {
    let email = email.strip_suffix('\n').unwrap_or(email);
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
    if !local_ok
        || !domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return false;
    }
    // The TLD has no dot, so only the last dot can start it.
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    !host.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// A record's `organizationUuid` compared as cswap's `account.get("organizationUuid", "") ==
/// org`: a missing one is `""`, a `null` one matches nothing.
fn org_equals(account: &serde_json::Map<String, Value>, org: &str) -> bool {
    match account.get("organizationUuid") {
        None => org.is_empty(),
        Some(Value::String(stored)) => stored == org,
        Some(_) => false,
    }
}

impl Switcher {
    /// `_get_current_account`: the live `(email, organizationUuid)`.
    pub(crate) fn get_current_account(&self) -> Option<(String, String)> {
        self.get_current_identity_triple()
            .map(|identity| (identity.email, identity.organization_uuid))
    }

    /// `_get_current_identity_triple`: the live email, `organizationUuid` and `accountUuid`
    /// from ONE read of the global config, so an identity checked and an identity stored can't
    /// come from two reads a `/login` landed between. `None` without an `oauthAccount` email.
    pub(crate) fn get_current_identity_triple(&self) -> Option<SlotIdentity> {
        let oauth_account = self.store.read_oauth_account()?;
        let field = |key: &str| text(Some(&oauth_account), key).to_owned();
        let email = field("emailAddress");
        if email.is_empty() {
            return None;
        }
        Some(SlotIdentity {
            email,
            organization_uuid: field("organizationUuid"),
            uuid: field("accountUuid"),
        })
    }

    /// The slot of loaded `data` holding the live login `live`, cswap's `_find_account_slot`
    /// of `_get_current_account()`: `None` without a login or for one no slot holds.
    pub(crate) fn live_slot(data: &Data, live: Option<&SlotIdentity>) -> Option<String> {
        let live = live?;
        Switcher::find_account_slot(data, &live.email, &live.organization_uuid)
    }

    /// `_find_account_slot`: the slot holding `(email, organization_uuid)`, in the roster's
    /// order.
    pub(crate) fn find_account_slot(
        data: &Data,
        email: &str,
        organization_uuid: &str,
    ) -> Option<String> {
        accounts(data)?.iter().find_map(|(num, account)| {
            let account = account.as_object()?;
            (account.get("email").and_then(Value::as_str) == Some(email)
                && org_equals(account, organization_uuid))
            .then(|| num.clone())
        })
    }

    /// `_account_exists`.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`].
    pub(crate) fn account_exists(&self, email: &str, organization_uuid: &str) -> Result<bool> {
        Ok(self.get_sequence_data()?.is_some_and(|data| {
            Switcher::find_account_slot(&data, email, organization_uuid).is_some()
        }))
    }

    /// `account_email`: the slot's stored email, `""` when unknown.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`].
    pub fn account_email(&self, account_num: &str) -> Result<String> {
        let data = self.sequence_data_or_empty()?;
        Ok(text(record(&data, account_num), "email").to_owned())
    }

    /// `account_identity`: the slot's stored email, organisation and uuid (trimmed).
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`].
    pub fn account_identity(&self, account_num: &str) -> Result<SlotIdentity> {
        let data = self.sequence_data_or_empty()?;
        let account = record(&data, account_num);
        Ok(SlotIdentity {
            email: text(account, "email").to_owned(),
            organization_uuid: text(account, "organizationUuid").to_owned(),
            uuid: text(account, "uuid").trim().to_owned(),
        })
    }

    /// `current_account_number`: the slot of the live login; `None` when there is none or it
    /// isn't managed. Never the recorded `activeAccountNumber`: an unmanaged login must not be
    /// taken for a slot, or auto-switch would read the wrong account's usage and overwrite a
    /// login Koyo doesn't own.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`].
    pub fn current_account_number(&self) -> Result<Option<String>> {
        let Some(live) = self.get_current_identity_triple() else {
            return Ok(None);
        };
        let data = self.sequence_data_or_empty()?;
        Ok(Switcher::live_slot(&data, Some(&live)))
    }

    /// `has_live_login`: the global config carries a live identity.
    pub fn has_live_login(&self) -> bool {
        self.get_current_account().is_some()
    }

    /// `backfill_account_uuid`: records a resolved uuid on a slot that has none, never over an
    /// existing one. With `expected_email` / `expected_org`, only while the slot still holds that
    /// identity under the lock: a remove and re-add in the gap must not get its predecessor's
    /// uuid. Takes Koyo's lock: the caller must not hold it.
    ///
    /// # Errors
    /// [`AccountsError::Lock`]; as [`Switcher::get_sequence_data`] and the write.
    pub fn backfill_account_uuid(
        &self,
        account_num: &str,
        uuid: &str,
        expected_email: Option<&str>,
        expected_org: Option<&str>,
    ) -> Result<()> {
        if uuid.is_empty() {
            return Ok(());
        }
        let _lock = FileLock::hold(&self.env, &self.lock_file, self.lock_timeout)?;
        let mut data = self.sequence_data_or_empty()?;
        let Some(account) = record_mut(&mut data, account_num) else {
            return Ok(());
        };
        let fills = text(Some(account), "uuid").trim().is_empty()
            && expected_email
                .is_none_or(|email| account.get("email").and_then(Value::as_str) == Some(email))
            && expected_org.is_none_or(|org| text(Some(account), "organizationUuid") == org);
        if fills {
            account.insert("uuid".into(), uuid.into());
            self.save_sequence(&mut data)?;
        }
        Ok(())
    }

    /// `_lineage_key`: the [`Switcher::probe_verdict`] key for a lineage, bound to the caller's
    /// email and the slot's full stored identity, so a slot re-created for another account
    /// misses its predecessor's verdicts. Built fresh at every consult.
    ///
    /// # Errors
    /// As [`Switcher::account_identity`].
    pub(crate) fn lineage_key(
        &self,
        account_num: &str,
        email: &str,
        fingerprint: &str,
    ) -> Result<LineageKey> {
        Ok(LineageKey {
            slot: account_num.to_owned(),
            caller_email: email.to_owned(),
            identity: self.account_identity(account_num)?,
            fingerprint: fingerprint.to_owned(),
        })
    }

    /// The cached verdict on `creds`' lineage for slot `account_num` and caller `email`
    /// ([`Switcher::lineage_key`], rebuilt at every consult, so a slot re-created for another
    /// account misses its predecessor's).
    ///
    /// # Errors
    /// As [`Switcher::account_identity`].
    pub(crate) fn lineage_verdict(
        &self,
        account_num: &str,
        email: &str,
        creds: &str,
    ) -> Result<Option<bool>> {
        let fingerprint = credential_fingerprint(creds).unwrap_or_default();
        Ok(self.probe_verdict(&self.lineage_key(account_num, email, &fingerprint)?))
    }

    /// Records a definitive verdict on `creds`' lineage, keyed as [`Switcher::lineage_verdict`]
    /// reads it.
    ///
    /// # Errors
    /// As [`Switcher::account_identity`].
    pub(crate) fn record_lineage_verdict(
        &self,
        account_num: &str,
        email: &str,
        creds: &str,
        verdict: bool,
    ) -> Result<()> {
        let fingerprint = credential_fingerprint(creds).unwrap_or_default();
        self.set_probe_verdict(self.lineage_key(account_num, email, &fingerprint)?, verdict);
        Ok(())
    }

    /// `_resolve_account_identifier`: a slot number as it is, an email to its one slot, `None`
    /// for an email no slot holds.
    ///
    /// # Errors
    /// [`AccountsError::Config`] for an email two slots share; as
    /// [`Switcher::get_sequence_data`].
    pub(crate) fn resolve_account_identifier(&self, identifier: &str) -> Result<Option<String>> {
        if is_digits(identifier) {
            return Ok(Some(identifier.to_owned()));
        }
        let Some(data) = self.get_sequence_data()? else {
            return Ok(None);
        };
        let matches: Vec<&String> = accounts(&data)
            .into_iter()
            .flatten()
            .filter(|(_, account)| account.get("email").and_then(Value::as_str) == Some(identifier))
            .map(|(num, _)| num)
            .collect();
        match matches.as_slice() {
            [] => Ok(None),
            [num] => Ok(Some((*num).clone())),
            many => {
                let details: Vec<String> = many
                    .iter()
                    .map(|num| {
                        let name = text(record(&data, num), "organizationName");
                        format!("{num} [{}]", org_label(name))
                    })
                    .collect();
                Err(AccountsError::Config(format!(
                    "Email '{identifier}' is ambiguous — matches accounts: {}. Use the account \
                     number instead.",
                    details.join(", ")
                )))
            }
        }
    }

    /// `resolve_account`: an identifier to `(slot, email)` (cswap's also returns the slot's
    /// `organizationUuid`, which its one ported caller ignores).
    ///
    /// # Errors
    /// [`AccountsError::AccountNotFound`] when it names no account; as
    /// [`Switcher::resolve_account_identifier`].
    pub(crate) fn resolve_account(&self, identifier: &str) -> Result<(String, String)> {
        let Some(account_num) = self.resolve_account_identifier(identifier)? else {
            return Err(AccountsError::AccountNotFound(format!(
                "No account found with identifier: {identifier}"
            )));
        };
        let data = self.sequence_data_or_empty()?;
        let Some(account) = record(&data, &account_num).filter(|account| !account.is_empty())
        else {
            return Err(AccountsError::AccountNotFound(format!(
                "Account-{account_num} does not exist"
            )));
        };
        let email = text(Some(account), "email").to_owned();
        Ok((account_num, email))
    }
}

/// Two credentials of one lineage: the same bytes or the same refresh-token fingerprint.
pub(crate) fn same_lineage(left: &str, right: &str) -> bool {
    left == right || credential_fingerprint(left) == credential_fingerprint(right)
}

/// Python's `str.isdigit()` for an identifier: non-empty, digits only.
pub(crate) fn is_digits(identifier: &str) -> bool {
    !identifier.is_empty() && identifier.chars().all(|c| c.is_ascii_digit())
}
