//! Port of claude-swap's `switcher.py` (0fe22a8), `_perform_switch`: the switch under the
//! locks, with its transaction and rollback, and the reads and writes it is made of
//! (`_read_target_credentials`, `_prepare_credentials_for_activation`).
//!
//! The live credential's owner is resolved first, before any lock (it may take the network).
//! Then Koyo's own `.lock`, Claude Code's credential locks and its config lock are held for the
//! whole mutation, rollback included: a Claude Code mid-refresh either finishes before the swap
//! (the backup captures its rotation) or re-checks after it, and the `oauthAccount` splice
//! can't interleave with Claude Code's own config writes. Everything under them is local.
//!
//! The normal path: the outgoing credential is backed up as its verdict allows
//! (`classify.rs`), the target's backup is activated (the machine-shared MCP fields taken from
//! the live credential), its `oauthAccount` spliced into the live config, and the roster's
//! active slot moved. A failure after the first write rolls every completed step back. The live
//! config is read once, as the transaction's snapshot, and that text (parsed once) is what the
//! safety copy's evidence and the splice use. With no live login or an unmanaged one, the
//! direct activation runs instead (`direct.rs`).
//!
//! Left out: the session-mode drift check at the start, `--force`, `_refuse_session_shell` and
//! the post-switch display; the remedies name Koyo's add rather than cswap's commands.

use std::path::Path;

use serde_json::Value;

use crate::claude_locks::{claude_config_lock, claude_credentials_lock};
use crate::credentials::{merge_shared_credential_fields, shared_credential_fields};
use crate::locking::{DEFAULT_TIMEOUT, FileLock};
use crate::oauth::value::truthy_at;
use crate::switcher::Switcher;
use crate::switcher::store::{
    Data, parse_object, read_config_text, record, record_mut, set_active, slot_int, text,
};
use crate::switcher::switch::rollback::{Originals, Step, SwitchTransaction};
use crate::switcher::switch::{AccountRef, Outgoing, OutgoingKind, Provenance, SwitchOp};
use crate::{AccountsError, Result};

/// The remedy for a slot whose backup is missing, in place of cswap's `--add-account --slot`.
pub(crate) fn readd_remedy(account_num: &str) -> String {
    format!("Log in as Account-{account_num}'s account in Claude Code, then add the current login.")
}

/// A config backup's `oauthAccount`, or `None` when it's missing or empty.
pub(crate) fn backup_oauth(config: &Value) -> Option<&Value> {
    config
        .as_object()
        .and_then(|config| config.get("oauthAccount"))
        .filter(|oauth| truthy_at(Some(oauth)))
}

impl Switcher {
    /// `_perform_switch`: switches to `target`, as the module says, and re-plans its usage
    /// poll once the locks are released. `provenance` is the pre-lock resolution when the
    /// caller already made it (a self-switch reconciling).
    ///
    /// # Errors
    /// [`AccountsError::Lock`] or [`AccountsError::ClaudeCodeLockTimeout`] before anything
    /// changed; [`AccountsError::CredentialRead`] for a live credential that can't be read or
    /// is empty; [`AccountsError::Switch`] for a target that can't be activated or a failure
    /// rolled back; the first failure itself when nothing was written yet.
    pub(crate) fn perform_switch(
        &self,
        target: &str,
        provenance: Option<Provenance>,
    ) -> Result<SwitchOp> {
        // Before the locks: it may hit the network.
        let provenance = match provenance {
            Some(provenance) => provenance,
            None => self.prefetch_live_identity()?,
        };
        let (op, org_uuid) = {
            let _lock = FileLock::hold(&self.env, &self.lock_file, DEFAULT_TIMEOUT)?;
            let _credentials_lock = claude_credentials_lock(&self.env, self.claude_lock_timeout)?;
            let _config_lock = claude_config_lock(&self.env, self.claude_lock_timeout)?;
            self.switch_locked(target, &provenance)?
        };
        self.replan_new_active(target, &op.to.email, &org_uuid);
        Ok(op)
    }

    /// The switch under the locks: the op, and the target's organisation for the re-plan.
    fn switch_locked(&self, target: &str, provenance: &Provenance) -> Result<(SwitchOp, String)> {
        let Some(mut data) = self.get_sequence_data()? else {
            return Err(AccountsError::Config("No accounts are managed yet".into()));
        };
        let Some(target_record) = record(&data, target) else {
            return Err(AccountsError::AccountNotFound(format!(
                "Account-{target} does not exist"
            )));
        };
        let target_email = text(Some(target_record), "email").to_owned();
        let to = AccountRef::new(Some(slot_int(target)?), &target_email);
        let current_identity = self.get_current_account();
        // The slot the live identity names; with no live identity, the recorded active slot.
        let current_account = match &current_identity {
            Some((email, org)) => Switcher::find_account_slot(&data, email, org),
            None => match data.get("activeAccountNumber") {
                None | Some(Value::Null) => None,
                Some(Value::String(active)) => Some(active.clone()),
                Some(active) => Some(active.to_string()),
            },
        };
        let (from, warnings) = match (current_identity, current_account) {
            (Some((current_email, _)), Some(current_account)) => {
                let from = AccountRef::new(Some(slot_int(&current_account)?), &current_email);
                let outgoing = Outgoing {
                    account: &current_account,
                    email: &current_email,
                    provenance,
                };
                let warnings = self.switch_normally(target, &target_email, &outgoing, &mut data)?;
                (Some(from), warnings)
            }
            (identity, current_account) => {
                // No live login yet, or one Koyo doesn't manage: there is nothing to back up
                // into, so the backup step is skipped and the displaced credential stashed.
                let from = identity.map(|(email, _)| AccountRef::new(None, &email));
                let warnings = self.activate_directly(
                    target,
                    &target_email,
                    current_account.as_deref(),
                    from.is_none(),
                    &mut data,
                )?;
                (from, warnings)
            }
        };
        let org_uuid = text(record(&data, target), "organizationUuid").to_owned();
        Ok((SwitchOp { from, to, warnings }, org_uuid))
    }

    /// The normal path's transaction around [`Switcher::switch_steps`]: the warnings for the
    /// result once it succeeded.
    fn switch_normally(
        &self,
        target: &str,
        target_email: &str,
        outgoing: &Outgoing<'_>,
        data: &mut Data,
    ) -> Result<Vec<String>> {
        let config_path = self.claude_config_path();
        let Some(credentials) = self.store.read_credentials() else {
            return Err(AccountsError::CredentialRead(
                "Failed to read current credentials".into(),
            ));
        };
        if credentials.is_empty() {
            // An empty read (a `security` timeout answers "" rather than failing) must never
            // be written over the departing account's backup.
            return Err(AccountsError::CredentialRead(
                "Current account credential is empty (Keychain unreadable?); refusing to \
                 overwrite its backup"
                    .into(),
            ));
        }
        let config = read_config_text(&config_path)?;
        let mut transaction = SwitchTransaction {
            originals: Originals {
                credentials,
                config,
                account_num: outgoing.account.to_owned(),
                config_path,
            },
            completed_steps: Vec::new(),
        };
        let steps = self.switch_steps(
            target,
            target_email,
            outgoing,
            &transaction.originals,
            data,
            &mut transaction.completed_steps,
        );
        let error = match steps {
            Ok(warnings) => {
                log::info!("Switched from account {} to {target}", outgoing.account);
                return Ok(warnings);
            }
            Err(error) => error,
        };
        log::error!("Switch failed: {error}, attempting rollback");
        if transaction.completed_steps.is_empty() {
            return Err(error);
        }
        if transaction.rollback(self) {
            log::info!("Rollback successful");
            Err(AccountsError::Switch(format!(
                "Switch failed and was rolled back: {error}"
            )))
        } else {
            log::error!("Rollback failed!");
            Err(AccountsError::Switch(format!(
                "Switch failed and rollback also failed: {error}. Manual recovery may be needed."
            )))
        }
    }

    /// The normal path's steps: back up the outgoing slot, activate the target's credential,
    /// splice its identity into the live config, move the active slot. Each write that can be
    /// undone is recorded in `completed`; the warnings are returned.
    fn switch_steps(
        &self,
        target: &str,
        target_email: &str,
        outgoing: &Outgoing<'_>,
        originals: &Originals,
        data: &mut Data,
        completed: &mut Vec<Step>,
    ) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        // The snapshot's config, for the safety copy's evidence and the splice.
        let live_config = parse_object(&originals.config_path, originals.config.as_bytes()).ok();

        // Step 1: back up the current account, as its verdict allows.
        let kind = self.classify_outgoing_credential(outgoing, &originals.credentials, data)?;
        self.back_up_outgoing(
            &kind,
            outgoing,
            originals,
            live_config.as_ref(),
            data,
            &mut warnings,
        )?;

        // Step 2: the target, read after the rescue (which may have advanced it).
        let target_creds = self.read_target_credentials(target, target_email)?;
        let target_config = self.read_account_config(target, target_email)?;
        if target_config.is_empty() {
            return Err(AccountsError::Switch(format!(
                "Account-{target} has no stored config backup. {}",
                readd_remedy(target)
            )));
        }

        // Step 3: the target's credential.
        self.store
            .write_credentials(&Switcher::prepare_credentials_for_activation(
                &target_creds,
                Some(&originals.credentials),
            ))?;
        completed.push(Step::CredentialsWritten);
        log::info!("Wrote target credentials");

        // Step 4: its identity spliced into the live config.
        let target_config_data: Value = serde_json::from_str(&target_config)
            .map_err(|error| AccountsError::Config(format!("Invalid backup config: {error}")))?;
        let Some(oauth_section) = backup_oauth(&target_config_data).cloned() else {
            return Err(AccountsError::Switch(
                "Invalid oauthAccount in backup".into(),
            ));
        };
        self.splice_live_config(
            &originals.config_path,
            live_config,
            oauth_section,
            &target_config_data,
            &mut warnings,
        )?;
        completed.push(Step::ConfigWritten);
        log::info!("Updated config file");

        // Step 5: the roster.
        set_active(data, target)?;
        self.save_sequence(data)?;
        completed.push(Step::SequenceUpdated);
        Ok(warnings)
    }

    /// Writes the target's `oauthAccount` into the live config (`live_config`, its snapshot
    /// parsed), keeping every other key; a config that is absent or can't be parsed is
    /// replaced by the target's backup, a copy of the unreadable one kept beside it first
    /// (`_salvage_unreadable`).
    pub(crate) fn splice_live_config(
        &self,
        config_path: &Path,
        live_config: Option<Data>,
        oauth_section: Value,
        target_config_data: &Value,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        // `is not None`, not truthiness: a valid `{}` loses nothing by being spliced.
        if let Some(mut current) = live_config {
            current.insert("oauthAccount".into(), oauth_section);
            return self.write_json(config_path, &current);
        }
        if config_path.exists() {
            self.salvage_unreadable(config_path, warnings)?;
        }
        self.write_json(config_path, target_config_data)
    }

    /// Step 1 by verdict: what may be written into the outgoing slot, and what is preserved.
    fn back_up_outgoing(
        &self,
        kind: &OutgoingKind,
        outgoing: &Outgoing<'_>,
        originals: &Originals,
        live_config: Option<&Data>,
        data: &mut Data,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        let (c, email) = (outgoing.account, outgoing.email);
        let (creds, config) = (originals.credentials.as_str(), originals.config.as_str());
        // Positively not this slot's bytes: never into it, never destroyed. The copy (which
        // fails the switch when it can't be made) licenses going on.
        let stash = || {
            self.stash_live_credential(
                creds,
                kind.as_str(),
                c,
                outgoing.provenance.resolved.as_ref(),
                live_config,
            )
        };
        match kind {
            OutgoingKind::Foreign(foreign) => {
                stash()?;
                let f = &foreign.num;
                warnings.push(if self.rescue_foreign_successor(foreign, data, creds)? {
                    format!(
                        "Credential ownership mismatch detected. The live credential belongs to \
                         Account-{f} and is newer than its stored backup, so Account-{f}'s \
                         backup was updated to it (a safety copy is kept). Nothing was written \
                         into Account-{c}."
                    )
                } else {
                    format!(
                        "Credential ownership mismatch detected. The live credential was \
                         preserved and was not written into Account-{c}. If Account-{f} later \
                         cannot authenticate, log in as it in Claude Code and add the current \
                         login again."
                    )
                });
            }
            OutgoingKind::KnownForeign => {
                stash()?;
                warnings.push(format!(
                    "The live credential was previously identified as another account's. It \
                     was preserved and not written into Account-{c}. If the owning account \
                     later cannot authenticate, log in as it in Claude Code and add the \
                     current login."
                ));
            }
            OutgoingKind::Alien => {
                stash()?;
                warnings.push(format!(
                    "The live login does not match a managed account. It was preserved and not \
                     written into Account-{c}. If you need that account, log in as it in \
                     Claude Code and add the current login."
                ));
            }
            OutgoingKind::ForeignSynced(f) => warnings.push(format!(
                "Credential ownership mismatch detected. The live credential already matches \
                 Account-{f}'s stored backup, so nothing was written into Account-{c}."
            )),
            OutgoingKind::Wiped | OutgoingKind::OauthAbsent => {
                // Nothing to preserve live, and writing it would replace the slot's only
                // refresh token: the config alone, the slot's backup is the recovery path.
                self.write_account_config(c, email, config)?;
                let cause = if *kind == OutgoingKind::Wiped {
                    "The live credential's tokens were wiped (Claude Code clears them when a \
                     refresh is rejected)."
                } else {
                    "The live credential holds no Claude login (an API key, or an emptied \
                     credentials store)."
                };
                warnings.push(format!(
                    "{cause} Account-{c}'s stored backup was kept. If the account cannot \
                     authenticate after switching back, log in with Claude Code and add the \
                     current login."
                ));
            }
            OutgoingKind::Unresolved => {
                // Fail open: the exact pre-fix backup. Most such divergences are the account's
                // own rotation, and the `.prev` kept by the write cushions a wrong call. Logged
                // only: a warning would cry wolf over a legitimate rotation.
                self.store.write_account_credentials(c, email, creds)?;
                self.write_account_config(c, email, config)?;
                log::info!(
                    "Backed up account {c} (lineage differs from the stored backup and \
                     ownership could not be verified — pre-fix backup)"
                );
            }
            OutgoingKind::OwnBytes => {
                self.write_account_config(c, email, config)?;
                log::info!("Backed up account {c} (config only; credentials unchanged)");
            }
            OutgoingKind::OwnFamily | OutgoingKind::OwnRotated => {
                self.store.write_account_credentials(c, email, creds)?;
                self.write_account_config(c, email, config)?;
                if *kind == OutgoingKind::OwnRotated {
                    // The profile proved the identity: a missing slot uuid is filled while the
                    // roster is being rewritten anyway.
                    backfill_resolved_uuid(data, c, outgoing.provenance);
                }
                log::info!("Backed up account {c}");
            }
        }
        Ok(())
    }

    /// `_read_target_credentials`: the target's stored credential, or why it can't be had.
    ///
    /// # Errors
    /// [`AccountsError::Switch`]: a backup the Keychain can't read right now (retry, don't
    /// re-add: a re-add would burn the stored grant), or none.
    pub(crate) fn read_target_credentials(&self, account_num: &str, email: &str) -> Result<String> {
        let (creds, unreadable) = self
            .store
            .read_account_credentials_ex(account_num, email, false);
        if !creds.is_empty() {
            return Ok(creds);
        }
        if unreadable {
            return Err(AccountsError::Switch(format!(
                "Account-{account_num}'s backup is in the macOS Keychain but it is unreadable \
                 right now (locked or no GUI session). Retry from a GUI session; do not re-add."
            )));
        }
        Err(AccountsError::Switch(format!(
            "Account-{account_num} has no stored credentials. {}",
            readd_remedy(account_num)
        )))
    }

    /// `_prepare_credentials_for_activation`: the target's stored credential with the live
    /// credential's machine-shared fields (`credentials::SHARED_CREDENTIAL_KEYS`, MCP sign-ins
    /// above all), absence included; everything else stays the target's. With no live JSON
    /// credential to take them from, the stored one activates unchanged.
    pub(crate) fn prepare_credentials_for_activation(target: &str, live: Option<&str>) -> String {
        match shared_credential_fields(live) {
            Some(shared) => merge_shared_credential_fields(target, &shared),
            None => target.to_owned(),
        }
    }
}

/// Fills an empty slot uuid from the profile that proved the rotation was the slot's own.
fn backfill_resolved_uuid(data: &mut Data, account_num: &str, provenance: &Provenance) {
    let Some(resolved_uuid) = provenance
        .resolved
        .as_ref()
        .map(|resolved| resolved.uuid.as_str())
        .filter(|uuid| !uuid.is_empty())
    else {
        return;
    };
    if let Some(account) = record_mut(data, account_num)
        && !truthy_at(account.get("uuid"))
    {
        account.insert("uuid".into(), resolved_uuid.into());
    }
}
