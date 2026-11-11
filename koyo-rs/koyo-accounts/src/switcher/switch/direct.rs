//! Port of claude-swap's `switcher.py` (0fe22a8), `_perform_switch`'s direct activation: the
//! target's backup written over the live login without a backup step, when there is no live
//! login yet or Koyo doesn't manage it (nothing to back it up into: the step would write
//! `account-None-*` backups).
//!
//! The live state is snapshotted first, config identity or not (a wiped config can orphan a
//! live credential whose MCP state must still reach the composer and the rollback); an
//! unreadable snapshot fails before anything is overwritten. The live credential it replaces
//! would otherwise have no surviving copy, so it is stashed first (cswap #117's invariant II);
//! no copy, no activation. A failure part way restores the config (by rename) and the
//! credential.
//!
//! Left out: `--force`, which also routes a managed live login here and lets a failed stash
//! proceed with a warning.

use serde_json::Value;

use crate::switcher::Switcher;
use crate::switcher::store::{Data, parse_object, set_active};
use crate::switcher::switch::perform::{backup_oauth, readd_remedy};
use crate::{AccountsError, Result, fsutil};

impl Switcher {
    /// The direct activation of `target`, under the switch's locks: the warnings for the
    /// result. `fresh_machine` is a machine with no live identity at all; `current_account` is
    /// the roster's recorded active slot then, the stash's `configSlot`.
    pub(super) fn activate_directly(
        &self,
        target: &str,
        target_email: &str,
        current_account: Option<&str>,
        fresh_machine: bool,
        data: &mut Data,
    ) -> Result<Vec<String>> {
        let config_path = self.claude_config_path();
        let target_creds = self.read_target_credentials(target, target_email)?;
        let target_config = self.read_account_config(target, target_email)?;
        if target_config.is_empty() {
            return Err(AccountsError::Switch(format!(
                "Account-{target} has no stored config backup. {}",
                readd_remedy(target)
            )));
        }
        let target_config_data: Value = serde_json::from_str(&target_config)
            .map_err(|error| AccountsError::Switch(format!("Invalid backup config: {error}")))?;
        let Some(target_oauth) = backup_oauth(&target_config_data).cloned() else {
            return Err(AccountsError::Switch(
                "Invalid oauthAccount in backup".into(),
            ));
        };

        // The snapshot, for the composer and the rollback. `None` is a credentials file that
        // exists but couldn't be read: no safety copy is possible, so nothing is overwritten.
        let Some(live_creds) = self.store.read_credentials() else {
            return Err(AccountsError::CredentialRead(
                "Cannot snapshot live credentials before activation".into(),
            ));
        };
        // On a fresh machine "" is "nothing to preserve" for the stash, the composer and the
        // rollback.
        let rollback_creds = (!fresh_machine || !live_creds.is_empty()).then_some(live_creds);
        let rollback_config_text = if config_path.exists() {
            Some(std::fs::read_to_string(&config_path).map_err(|error| {
                AccountsError::Config(format!(
                    "Cannot snapshot live config before activation: {error}"
                ))
            })?)
        } else {
            None
        };
        // Parsed once, for the safety copy's evidence and the splice.
        let live_config = rollback_config_text
            .as_deref()
            .and_then(|text| parse_object(&config_path, text.as_bytes()).ok());

        if let Some(live) = rollback_creds.as_deref()
            && !live.is_empty()
            && live != target_creds
        {
            self.stash_live_credential(
                live,
                "displaced-live-login",
                current_account.unwrap_or("unmanaged"),
                None,
                live_config.as_ref(),
            )
            .map_err(|error| {
                AccountsError::Switch(format!(
                    "Could not preserve the live credential before activation (safety-copy \
                     write failed: {error}); aborting rather than destroying it"
                ))
            })?;
        }

        // A failure part way restores what was written: the config (by rename), then the
        // credential.
        let restore_config = || {
            if let Some(text) = &rollback_config_text
                && let Err(error) = fsutil::atomic_write(&self.env, &config_path, text.as_bytes())
            {
                log::error!("Failed to rollback config: {error}");
            }
        };
        let restore_credentials = || {
            if let Some(creds) = &rollback_creds
                && let Err(error) = self.store.write_credentials(creds)
            {
                log::error!("Failed to rollback credentials: {error}");
            }
        };

        self.store
            .write_credentials(&Switcher::prepare_credentials_for_activation(
                &target_creds,
                rollback_creds.as_deref(),
            ))?;
        // Keep the live settings and projects, swapping in `oauthAccount`; a config that is
        // absent or torn is replaced by the backup, the torn bytes kept beside it.
        let mut warnings = Vec::new();
        if let Err(error) = self.splice_live_config(
            &config_path,
            live_config,
            target_oauth,
            &target_config_data,
            &mut warnings,
        ) {
            restore_credentials();
            return Err(error);
        }
        if let Err(error) = set_active(data, target).and_then(|()| self.save_sequence(data)) {
            restore_config();
            restore_credentials();
            return Err(error);
        }
        log::info!("Activated account {target} (no prior live account)");
        Ok(warnings)
    }
}
