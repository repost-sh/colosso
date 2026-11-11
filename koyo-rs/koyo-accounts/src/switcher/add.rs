//! Port of claude-swap's `switcher.py` (0fe22a8), `add_account`: the current login into a
//! slot, the account's own when it is already managed (the credential refreshed in place, its
//! settings kept), else the next new one; with the guards that keep a slot from being LABELLED
//! one account and CONTAINING another.
//!
//! The identity comes from the global config, the credential from the profile's store
//! (`capture.rs`). Then, in this order, before anything is written: an API key is refused, the
//! token is resolved over the network (`oauth::fetch_oauth_profile`, with no lock held) and a
//! token that resolves to another account is refused, the store is read again and a lineage
//! that rotated meanwhile is refused, the config's bytes are read, and the identity is read
//! again and one that changed meanwhile is refused. The ownership check is advisory: an answer
//! that can't be had (offline, a 401, an expired access token, which is never refreshed here
//! since spending a grant is a coordinated transition) registers anyway and says so; only an
//! identity that disagrees refuses.
//!
//! Left out: the `slot` and `alias` arguments, the overwrite prompt, the displacement of
//! another account's slot and the move of an account between slots, which only `--slot` reaches
//! (Koyo adds the current login, `docs/claude-accounts.md` 6); `_refuse_session_shell` and the
//! org-field migration.

use serde_json::{Map, Value, json};

use crate::credentials::looks_like_api_key;
use crate::oauth::{
    self, credential_fingerprint, extract_access_token, extract_oauth_data, has_login_token,
};
use crate::switcher::Switcher;
use crate::switcher::identity::{SlotIdentity, org_label};
use crate::switcher::store::{
    Data, get_timestamp, parse_object, read_config_text, record, set_active, slot_int, text,
};
use crate::usage_store::one_identity;
use crate::{AccountsError, Result};

/// What [`Switcher::add_account`] did, for the app to say what cswap prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddResult {
    pub account_num: String,
    pub email: String,
    pub organization_uuid: String,
    pub organization_name: String,
    /// The account was managed already: its credential and config were refreshed in place.
    pub updated: bool,
    /// What cswap prints as a notice: the ownership check that couldn't complete.
    pub notices: Vec<String>,
}

impl Switcher {
    /// `add_account`, as the module says.
    ///
    /// # Errors
    /// [`AccountsError::Config`] with no live login, for a credential of another account (or
    /// none at all), or for a login or rotation that landed during the check;
    /// [`AccountsError::CredentialRead`] for a credential that couldn't be read or is missing;
    /// [`AccountsError::Validation`] for an API key; the store's and the file system's.
    pub fn add_account(&self) -> Result<AddResult> {
        self.setup_directories()?;
        self.init_sequence_file()?;
        let Some(identity) = self.get_current_identity_triple() else {
            return Err(AccountsError::Config(
                "No active Claude account found. Please log in first.".into(),
            ));
        };
        let email = identity.email.as_str();
        let mut notices = Vec::new();

        if self.account_exists(email, &identity.organization_uuid)? {
            // Already managed: refresh its credential and config in place.
            let mut seq = self.sequence_data_or_empty()?;
            let Some(account_num) =
                Switcher::find_account_slot(&seq, email, &identity.organization_uuid)
            else {
                return Err(AccountsError::AccountNotFound(format!(
                    "{email} is no longer managed"
                )));
            };
            let org_name = text(record(&seq, &account_num), "organizationName").to_owned();
            let creds = self.capture_verified(&identity, &mut notices)?;
            let config = read_config_text(&self.claude_config_path())?;
            // After the read, as it licenses those bytes, and on the identity that was read.
            self.reject_identity_drift_since_verify(&identity)?;
            self.store
                .write_account_credentials(&account_num, email, &creds)?;
            self.write_account_config(&account_num, email, &config)?;
            self.clear_dead_token(&account_num, email, &identity.organization_uuid)?;
            set_active(&mut seq, &account_num)?;
            self.save_sequence(&mut seq)?;
            log::info!("Updated credentials for account {account_num}: {email}");
            return Ok(AddResult {
                account_num,
                email: identity.email,
                organization_uuid: identity.organization_uuid,
                organization_name: org_name,
                updated: true,
                notices,
            });
        }

        let account_num = self.get_next_account_number()?.to_string();
        // Every read before anything destructive.
        let creds = self.capture_verified(&identity, &mut notices)?;
        let config_path = self.claude_config_path();
        let config = read_config_text(&config_path)?;
        // Strict: the ownership probe's round trip sits between the identity read and this
        // one, and a config caught mid-rewrite must refuse, not blank the slot's fields.
        let config_data =
            parse_object(&config_path, config.as_bytes()).map_err(AccountsError::Config)?;
        let oauth_account = config_data.get("oauthAccount").and_then(Value::as_object);
        let field = |key: &str| text(oauth_account, key).to_owned();
        let (uuid, organization_uuid, organization_name) = (
            field("accountUuid"),
            field("organizationUuid"),
            field("organizationName"),
        );
        self.reject_identity_drift_since_verify(&identity)?;

        self.store
            .write_account_credentials(&account_num, email, &creds)?;
        self.write_account_config(&account_num, email, &config)?;
        self.clear_dead_token(&account_num, email, &organization_uuid)?;

        let mut data = self.sequence_data_or_empty()?;
        let account = json!({
            "email": email,
            "uuid": uuid,
            "organizationUuid": organization_uuid,
            "organizationName": organization_name,
            "added": get_timestamp(&self.env),
        });
        insert_account(&mut data, &account_num, account);
        add_to_sequence(&mut data, slot_int(&account_num)?);
        set_active(&mut data, &account_num)?;
        self.save_sequence(&mut data)?;
        log::info!(
            "Added account {account_num}: {email} (org: {})",
            org_label(&organization_uuid)
        );
        Ok(AddResult {
            account_num,
            email: identity.email,
            organization_uuid,
            organization_name,
            updated: false,
            notices,
        })
    }

    /// The capture and its guards, shared by both branches of `add_account`: read, refuse an
    /// API key, check the owner, refuse a rotation since.
    fn capture_verified(
        &self,
        identity: &SlotIdentity,
        notices: &mut Vec<String>,
    ) -> Result<String> {
        let Some(creds) = self.read_capture_credentials()? else {
            return Err(AccountsError::CredentialRead(
                "Failed to read credentials for current account".into(),
            ));
        };
        if creds.is_empty() {
            return Err(AccountsError::CredentialRead(
                "No credentials found for current account".into(),
            ));
        }
        reject_live_api_key_capture(&creds)?;
        if let Some(notice) = self.reject_foreign_credential_capture(
            &creds,
            &identity.email,
            &identity.organization_uuid,
            &identity.uuid,
        )? {
            notices.push(notice);
        }
        self.reject_credential_drift_since_verify(&creds)?;
        Ok(creds)
    }

    fn clear_dead_token(&self, account_num: &str, email: &str, org_uuid: &str) -> Result<()> {
        self.usage_store
            .clear_dead_token(&[account_num], &one_identity(account_num, email, org_uuid))
    }

    /// `_reject_identity_drift_since_verify`: refuses when the live identity moved while the
    /// credential was being checked over the network: a `/login` in that window puts one
    /// account's identity on another's credential.
    ///
    /// # Errors
    /// [`AccountsError::Config`] when it moved.
    pub(crate) fn reject_identity_drift_since_verify(&self, verified: &SlotIdentity) -> Result<()> {
        let now = self.get_current_identity_triple();
        if now.as_ref() == Some(verified) {
            return Ok(());
        }
        let now_email = now.map_or_else(|| "unknown".into(), |now| now.email);
        Err(AccountsError::Config(format!(
            "The active account changed while {} was being verified (now {now_email}). Nothing \
             was changed. Re-run when no other login is in flight.",
            verified.email
        )))
    }

    /// `_reject_credential_drift_since_verify`: refuses when the stored lineage rotated during
    /// the check (a refresh of the same account moves no identity, only the credential):
    /// storing the pre-rotation bytes hands the slot a generation the server already retired.
    /// The fingerprint is the refresh token's, so an access-token-only rotation compares
    /// equal. A store that can't be read again is unverifiable, never a refusal.
    ///
    /// # Errors
    /// [`AccountsError::Config`] when the lineage advanced.
    pub(crate) fn reject_credential_drift_since_verify(&self, verified: &str) -> Result<()> {
        let Ok(Some(now)) = self.read_capture_credentials() else {
            return Ok(());
        };
        if now.is_empty() {
            return Ok(());
        }
        match (
            credential_fingerprint(verified),
            credential_fingerprint(&now),
        ) {
            (Some(before), Some(after)) if before != after => Err(AccountsError::Config(
                "The stored credential rotated while it was being verified. Nothing was changed. \
                 Registering the pre-rotation generation would hand the slot a credential the \
                 server has already retired. Re-run when no refresh is in flight."
                    .into(),
            )),
            _ => Ok(()),
        }
    }

    /// `_reject_foreign_credential_capture`: the stored token must be this account's, compared
    /// uuid first (the email decides only without a stored uuid), the organisation corroborated
    /// either way. `Ok(Some(notice))` when the check couldn't complete and the add goes on.
    ///
    /// # Errors
    /// [`AccountsError::Config`] for a credential with neither token (Claude Code empties both
    /// after a rejected refresh: provably unusable), or one that resolves to another account
    /// or organisation.
    pub(crate) fn reject_foreign_credential_capture(
        &self,
        creds: &str,
        email: &str,
        org_uuid: &str,
        account_uuid: &str,
    ) -> Result<Option<String>> {
        // Fail-open, but never silently: registering with the question unanswered is the
        // state the field incident was in.
        let unverified = |why: &str| {
            Ok(Some(format!(
                "Could not verify that the stored credential belongs to {email} ({why}). \
                 Registered anyway; re-run where the check can complete to confirm."
            )))
        };
        let oauth_data = extract_oauth_data(creds);
        if let Some(data) = &oauth_data
            && !has_login_token(data)
        {
            return Err(AccountsError::Config(format!(
                "The stored credential for {email} has no access or refresh token (Claude Code \
                 clears both after a rejected refresh). Nothing was changed. Log in as {email} \
                 again, then re-run."
            )));
        }
        let Some(token) = extract_access_token(creds).filter(|token| !token.is_empty()) else {
            return unverified("no access token to resolve");
        };
        if let Some(data) = oauth_data.as_ref().filter(|data| !data.is_empty())
            && oauth::is_oauth_token_expired(data.get("expiresAt"), self.env.now())
        {
            // Unresolvable, like an offline lookup; never a refresh, whose accepted rotation
            // retires the predecessor that the active store still holds.
            return unverified("the access token is expired");
        }
        let Some(profile) = oauth::fetch_oauth_profile(&self.env, &token) else {
            return unverified("the identity lookup did not resolve");
        };
        let seen_uuid = profile.uuid.trim();
        if account_uuid.is_empty() {
            let seen = profile.email.as_deref().unwrap_or_default().trim();
            if seen.is_empty() {
                return unverified("the resolved identity carries no address");
            }
            if seen.to_lowercase() != email.to_lowercase() {
                return Err(AccountsError::Config(format!(
                    "The stored credential does not belong to {email}: the token resolves to \
                     {seen}. Nothing was changed. This happens when the config names one account \
                     while the credential store still holds another's token (e.g. a renamed \
                     .claude.json over a live keychain item). Log in as {email} in THIS \
                     environment, then re-run."
                )));
            }
        } else if seen_uuid != account_uuid {
            // A match falls through to the organisation below: a uuid under a disagreeing org
            // is another account.
            return Err(AccountsError::Config(format!(
                "The stored credential does not belong to {email}: the token resolves to account \
                 {seen_uuid}, not {account_uuid}. Nothing was changed. This happens when the \
                 config names one account while the credential store still holds another's \
                 token (e.g. a renamed .claude.json over a live keychain item). Log in as \
                 {email} in THIS environment, then re-run."
            )));
        }
        // No organisation block at all is unverifiable about the org only.
        let Some(resolved_org) = profile.organization_uuid.as_deref() else {
            return Ok(None);
        };
        let seen_org = resolved_org.trim();
        if seen_org == org_uuid {
            return Ok(None);
        }
        Err(AccountsError::Config(format!(
            "The stored credential for {email} belongs to organization {}, not {}. Nothing was \
             changed. Two accounts can share an email across organizations. Log in as {email} \
             in the {} organization in THIS environment, then re-run.",
            org_label(seen_org),
            org_label(org_uuid),
            org_label(org_uuid),
        )))
    }
}

/// `_reject_live_api_key_capture`: a live managed API key is never captured as a login.
///
/// # Errors
/// [`AccountsError::Validation`] for an API key.
pub(crate) fn reject_live_api_key_capture(creds: &str) -> Result<()> {
    if looks_like_api_key(Some(creds)) {
        return Err(AccountsError::Validation(
            "Active login is an API-key account. Koyo adds Claude logins only.".into(),
        ));
    }
    Ok(())
}

/// `data["accounts"][num] = account`, the accounts object made if it's missing.
fn insert_account(data: &mut Data, account_num: &str, account: Value) {
    let accounts = data
        .entry("accounts")
        .or_insert_with(|| Value::Object(Map::new()));
    if !accounts.is_object() {
        *accounts = Value::Object(Map::new());
    }
    if let Some(accounts) = accounts.as_object_mut() {
        accounts.insert(account_num.to_owned(), account);
    }
}

/// `if num not in data["sequence"]: append; sort()`.
fn add_to_sequence(data: &mut Data, number: u64) {
    let sequence = data
        .entry("sequence")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !sequence.is_array() {
        *sequence = Value::Array(Vec::new());
    }
    if let Some(sequence) = sequence.as_array_mut()
        && !sequence.iter().any(|num| num.as_u64() == Some(number))
    {
        sequence.push(Value::from(number));
        sequence.sort_by_key(|num| num.as_u64().unwrap_or(u64::MAX));
    }
}
