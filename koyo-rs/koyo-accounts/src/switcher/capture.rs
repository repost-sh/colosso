//! Port of claude-swap's `switcher.py` (0fe22a8), the credential read `add_account` captures
//! (`_read_capture_credentials`, `_refuse_degraded_capture`), with the profile read it rests
//! on from `session.py` (`read_config_dir_credentials`, `_keychain_entry`; its
//! `_keychain_services` is `paths::keychain_services`), the one piece of session mode the
//! capture needs.
//!
//! An add fills one slot from two reads: the identity out of the global config and the
//! credential out of a store. They must come from the same profile, or the slot holds one
//! account's email against another's token. So the credential is read the way Claude Code
//! (2.1.220) resolves it for the same environment: `CLAUDE_SECURESTORAGE_CONFIG_DIR` when it is
//! defined (defined but empty is the default store), else `CLAUDE_CONFIG_DIR`. Identity stays on
//! `CLAUDE_CONFIG_DIR` either way.
//!
//! The read is strict on the Keychain: an unreadable (not absent) item fails the add rather
//! than capture the profile's plaintext seed, which may predate an in-profile `/login`. An
//! absent item (exit 44) falls back to `.credentials.json`, Claude Code's own signal. On the
//! default path the active read's own verdict decides: a degraded read (the Keychain failed and
//! the file covered it) is refused, the value checked being the value captured.

use std::path::Path;

use crate::credentials::{ACTIVE_READ_ATTEMPTS, ACTIVE_READ_RETRY_DELAY};
use crate::keychain::{self, KeychainError};
use crate::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE, keychain_services};
use crate::switcher::Switcher;
use crate::switcher::store::text;
use crate::{AccountsError, Result, fsutil};

impl Switcher {
    /// `_keychain_entry`: one service's item, `None` when absent; an unreadable Keychain is
    /// retried once when `strict` (cswap's `_STRICT_KEYCHAIN_*`, which mirror the active
    /// store's attempts and delay), then its error returned. A direct `security` call, as
    /// cswap's: a profile's failure is not the active store's to learn from.
    fn keychain_entry(&self, service: &str, strict: bool) -> Result<Option<String>, KeychainError> {
        let account = self.store.account_name();
        let mut attempts = if strict { ACTIVE_READ_ATTEMPTS } else { 1 };
        loop {
            match keychain::get_password(&*self.env.security, service, &account) {
                Ok(value) => return Ok(value),
                Err(error) => {
                    attempts -= 1;
                    if attempts == 0 {
                        return Err(error);
                    }
                    fsutil::nap(ACTIVE_READ_RETRY_DELAY);
                }
            }
        }
    }

    /// `read_config_dir_credentials`: the credential of the profile at `config_dir` (the
    /// variable's value as written, since Claude Code hashes it verbatim), its Keychain item(s)
    /// first, then its `.credentials.json`; `None` for a folder that isn't there or holds no
    /// readable credential. `service` overrides the hashed name for the default profile.
    ///
    /// # Errors
    /// With `strict_keychain`, [`AccountsError::CredentialRead`] for an unreadable Keychain;
    /// without, the read goes on to the file.
    pub(crate) fn read_config_dir_credentials(
        &self,
        config_dir: &str,
        strict_keychain: bool,
        service: Option<&str>,
    ) -> Result<Option<String>> {
        let directory = Path::new(config_dir);
        if !directory.is_dir() {
            return Ok(None);
        }
        for service in keychain_services(config_dir, service) {
            match self.keychain_entry(&service, strict_keychain) {
                Err(error) if strict_keychain => {
                    return Err(AccountsError::CredentialRead(format!(
                        "Keychain entry for profile {config_dir} is unreadable (locked or busy) \
                         — unlock the keychain and retry: {error}"
                    )));
                }
                // Best-effort: the plaintext seed is the next-best truth.
                Err(_) => break,
                Ok(Some(creds)) if !creds.is_empty() => return Ok(Some(creds)),
                // Absent: Claude Code's own signal to look further.
                Ok(_) => {}
            }
        }
        Ok(std::fs::read_to_string(directory.join(".credentials.json")).ok())
    }

    /// `_refuse_degraded_capture`: the active credential, unless the read that produced it was
    /// degraded (the Keychain failed and the plaintext file covered it, which on macOS may be a
    /// superseded generation, since Claude Code rotates Keychain-only). The value returned is
    /// that read's own, so the check and the capture can't be two reads.
    ///
    /// # Errors
    /// [`AccountsError::CredentialRead`] for a degraded read.
    pub(crate) fn refuse_degraded_capture(&self) -> Result<Option<String>> {
        let active = self.store.read_active_credentials();
        if active.degraded {
            return Err(AccountsError::CredentialRead(
                "The macOS Keychain is unreadable right now (locked or no GUI session), so the \
                 only readable credential is a plaintext fallback that may be a superseded \
                 generation — capturing it would file a spent refresh token against this slot. \
                 Retry from a GUI session."
                    .into(),
            ));
        }
        Ok(active.value)
    }

    /// `_read_capture_credentials`: the credential of the profile the environment points at,
    /// as the module says; `None` when it couldn't be read, `""` for none. Read-only.
    ///
    /// # Errors
    /// [`AccountsError::CredentialRead`] for an unreadable Keychain or a degraded read.
    pub(crate) fn read_capture_credentials(&self) -> Result<Option<String>> {
        if let Some(secure) = self.env.vars.securestorage_config_dir.as_deref() {
            // A defined override names the only store Claude Code reads here; on a miss it sees
            // a logged-out environment, so nothing falls back into another profile's store, nor
            // to the tail below, which follows `CLAUDE_CONFIG_DIR`.
            let dir = if secure.is_empty() {
                paths::default_claude_config_home(&self.env)
                    .to_string_lossy()
                    .into_owned()
            } else {
                secure.to_owned()
            };
            let service = secure.is_empty().then_some(CLAUDE_CODE_KEYCHAIN_SERVICE);
            let creds = self.read_config_dir_credentials(&dir, true, service)?;
            return Ok(Some(creds.unwrap_or_default()));
        }
        let Some(config_dir) = self.env.vars.config_dir() else {
            return self.refuse_degraded_capture();
        };
        if let Some(creds) = self
            .read_config_dir_credentials(config_dir, true, None)?
            .filter(|creds| !creds.is_empty())
        {
            return Ok(Some(creds));
        }
        if paths::same_directory(
            Path::new(config_dir),
            &paths::default_claude_config_home(&self.env),
        ) {
            // Only here do the active store's file and the default profile coincide.
            return self.refuse_degraded_capture();
        }
        // Only this profile's own `primaryApiKey`, never the default profile's Keychain key.
        let config = self.store.read_global_config();
        Ok(Some(text(config.as_ref(), "primaryApiKey").to_owned()))
    }
}
