//! Port of claude-swap's `credentials.py` (0fe22a8), its active-credential half: Claude
//! Code's OAuth login read with its verdicts, written to the Keychain, and cleared.
//!
//! The read tries the Keychain item(s) Claude Code reads for this environment, twice each with
//! 0.3 s between, then Claude Code's own `.credentials.json`. Exit 44 is an absent item, a
//! clean miss; any other failure is an unreadable Keychain, which marks what follows
//! degraded, and unavailable when nothing covered it. cswap then reads a managed API key;
//! that step is left out with managed keys.
//!
//! The write goes to the unsuffixed `Claude Code-credentials` item, as cswap's does whatever
//! the profile (it never writes Claude Code's hashed items). After it lands, an
//! already-present `.credentials.json` is rewritten with the same bytes: Claude Code drops its
//! memoised token only when that file's mtime changes, so a running session reloads at once
//! (#86), and a container sharing `~/.claude` stays consistent (#1414). The file is never
//! created: a Keychain-only Mac keeps no plaintext credential, and reloads within Claude
//! Code's 30 s Keychain cache. cswap writes the file as the store when the Keychain is
//! unusable; Koyo fails the write instead (the module's doc).

use crate::credentials::{
    ACTIVE_READ_ATTEMPTS, ACTIVE_READ_RETRY_DELAY, ActiveCredentials, CredentialStore,
    active_oauth_keychain_services, looks_like_api_key,
};
use crate::keychain::{self, KeychainError};
use crate::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE};
use crate::{AccountsError, Result, fsutil};

impl CredentialStore {
    /// The active credential's value alone: the credential, `""` when there's none, `None`
    /// when `.credentials.json` couldn't be read.
    pub fn read_credentials(&self) -> Option<String> {
        self.read_active_credentials().value
    }

    /// Reads the active profile's OAuth item(s) in order, stopping at the first value:
    /// `(value, failed)`, `failed` only for an unreadable Keychain. That stops the walk too:
    /// it's the Keychain's state, not the item's, so another name can't fare better.
    pub fn read_active_oauth_keychain(&self) -> (Option<String>, bool) {
        for service in active_oauth_keychain_services(&self.env) {
            let (value, failed) = self.read_one_oauth_keychain(&service);
            if value.as_deref().is_some_and(|value| !value.is_empty()) {
                return (value, false);
            }
            if failed {
                return (None, true);
            }
        }
        (None, false)
    }

    /// Reads one OAuth item, retried: `(value, failed)`, `failed` only when every attempt
    /// failed. An absent item (exit 44) isn't a failure and isn't retried.
    pub fn read_one_oauth_keychain(&self, service: &str) -> (Option<String>, bool) {
        let account = self.account_name();
        let mut last_error: Option<KeychainError> = None;
        for attempt in 0..ACTIVE_READ_ATTEMPTS {
            match self.kc_call(|security| keychain::get_password(security, service, &account)) {
                Ok(value) => return (value, false),
                Err(error) => {
                    last_error = Some(error);
                    if attempt + 1 < ACTIVE_READ_ATTEMPTS {
                        fsutil::nap(ACTIVE_READ_RETRY_DELAY);
                    }
                }
            }
        }
        log::warn!(
            "Keychain read failed after {ACTIVE_READ_ATTEMPTS} attempt(s), trying file: {}",
            last_error.map(|error| error.0).unwrap_or_default()
        );
        (None, true)
    }

    /// Reads Claude Code's active credential and says how: the OAuth Keychain item for this
    /// profile when the Keychain is in use, then `.credentials.json`. Changes nothing.
    ///
    /// The item read is this profile's, not the fixed name: Claude Code keeps only the
    /// default profile's credential under `Claude Code-credentials`, so reading it under a
    /// custom `CLAUDE_CONFIG_DIR` pairs one account's token with another's identity. Read, not
    /// skipped: Claude Code writes rotations Keychain-only there, so the file is often absent.
    pub fn read_active_credentials(&self) -> ActiveCredentials {
        let keychain_failed = if self.use_keychain() {
            let (value, failed) = self.read_active_oauth_keychain();
            // This read's own verdict, which a later success on another item can't erase.
            self.state().active_read_failed = failed;
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                return ActiveCredentials::new(Some(value), false, false);
            }
            failed
        } else {
            // Known unusable: what follows covers a Keychain that couldn't be asked.
            let active_read_failed = self.state().active_read_failed;
            active_read_failed || self.keychain_unreadable()
        };
        // Claude Code's own plaintext fallback. After a failed Keychain read it may hold a
        // superseded generation, which `degraded` carries.
        let path = paths::credentials_path(&self.env);
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Err(error) => {
                    log::error!("Failed to read credentials file: {error}");
                    // A denied Keychain and an unreadable file is the most unreadable state
                    // there is, not "no credentials".
                    return ActiveCredentials::new(None, keychain_failed, keychain_failed);
                }
                Ok(text) if !text.trim().is_empty() => {
                    return ActiveCredentials::new(Some(text), false, keychain_failed);
                }
                Ok(_) => {}
            }
        }
        ActiveCredentials::new(Some(String::new()), keychain_failed, keychain_failed)
    }

    /// Removes the active credential's Keychain item, best-effort: whether no item is left to
    /// shadow the file (a delete returns only on exit 0 or 44). Not a [`Self::kc_call`].
    pub fn delete_active_keychain_entry(&self) -> bool {
        keychain::delete_password(
            &*self.env.security,
            CLAUDE_CODE_KEYCHAIN_SERVICE,
            &self.account_name(),
        )
        .is_ok()
    }

    /// Writes Claude Code's active credential. An OAuth credential is written by
    /// [`Self::write_oauth_credentials`]; cswap then clears any managed API key, left out
    /// here with managed keys.
    ///
    /// # Errors
    /// [`AccountsError::Credential`] for a managed API key, which Koyo doesn't activate; as
    /// [`Self::write_oauth_credentials`].
    pub fn write_credentials(&self, credentials: &str) -> Result<()> {
        if looks_like_api_key(Some(credentials)) {
            return Err(AccountsError::Credential(
                "This credential is a managed API key; Koyo switches Claude logins only.".into(),
            ));
        }
        self.write_oauth_credentials(credentials)
    }

    /// Writes the active OAuth credential to the Keychain, then bumps an already-present
    /// `.credentials.json` (the module's doc).
    ///
    /// # Errors
    /// [`AccountsError::CredentialWrite`] when the Keychain is backed off or the write fails;
    /// nothing is written to a file in its place.
    pub fn write_oauth_credentials(&self, credentials: &str) -> Result<()> {
        if !self.use_keychain() {
            return Err(AccountsError::CredentialWrite(
                "Failed to write credentials: the Keychain is backed off after a failure, and \
                 Koyo keeps no plaintext copy to write instead. Retry in a minute."
                    .into(),
            ));
        }
        let account = self.account_name();
        if let Err(error) = self.kc_call(|security| {
            keychain::set_password(
                security,
                CLAUDE_CODE_KEYCHAIN_SERVICE,
                &account,
                credentials,
            )
        }) {
            log::warn!("Keychain write failed: {error}");
            return Err(AccountsError::CredentialWrite(format!(
                "Failed to write credentials: {error}"
            )));
        }
        self.refresh_stale_credentials_file(credentials);
        self.state().last_active_credentials_backend = Some("keychain");
        Ok(())
    }

    /// Rewrites an already-present `.credentials.json` with `credentials` after a Keychain
    /// write, so its mtime moves even when the bytes don't (the atomic rename). Never creates
    /// it. Best-effort: the Keychain write already landed, so a failure here only means a
    /// running session may lag until it restarts.
    pub fn refresh_stale_credentials_file(&self, credentials: &str) {
        if !paths::credentials_path(&self.env).exists() {
            return;
        }
        if let Err(error) = self.write_active_credentials_file(credentials) {
            log::warn!(
                "Could not refresh .credentials.json after Keychain write ({error}); a running \
                 session may not hot-reload until restart"
            );
        }
    }

    /// `.credentials.json`, written: the refresh's writer only, never a store.
    fn write_active_credentials_file(&self, credentials: &str) -> Result<()> {
        let dir = paths::claude_config_home(&self.env);
        fsutil::create_dir_all(&self.env, &dir)?;
        fsutil::atomic_write(
            &self.env,
            &dir.join(".credentials.json"),
            credentials.as_bytes(),
        )
    }

    /// Clears the active OAuth credential, the Keychain item and `.credentials.json`,
    /// best-effort: a down Keychain or a missing file is fine.
    pub fn clear_oauth_credential(&self) {
        self.delete_active_keychain_entry();
        let path = paths::credentials_path(&self.env);
        if path.exists()
            && let Err(error) = fsutil::remove_file(&self.env, &path)
        {
            log::warn!("Failed to remove credentials file: {error}");
        }
    }
}
