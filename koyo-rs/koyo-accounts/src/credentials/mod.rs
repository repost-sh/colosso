//! Port of claude-swap's `credentials.py` (0fe22a8): where Claude Code's active credential and
//! Koyo's per-account backups live, and how they are read and written. macOS only.
//!
//! - The active OAuth credential is the Keychain item Claude Code reads for this environment
//!   ([`active_oauth_keychain_services`]), read with a bounded retry. When the Keychain can't be
//!   read, Claude Code's own `<config home>/.credentials.json` is read in its place, as Claude
//!   Code itself falls back to it, and the answer says so ([`ActiveCredentials`]): bytes read
//!   that way may be a superseded generation, since Claude Code rotates Keychain-only on macOS.
//! - A write of the active credential goes to the Keychain and bumps an already-present
//!   `.credentials.json`, so a running Claude Code reloads at once (`active.rs`).
//! - Each account's backup is a Keychain item under [`paths::BACKUP_KEYCHAIN_SERVICE`], account
//!   `account-<n>-<email>`, with the previous generation kept beside it as `….prev`; display
//!   reads may be served from a one-minute cache (`backup.rs`).
//! - A failed Keychain call backs the Keychain off for [`KEYCHAIN_RECHECK_COOLDOWN_S`]: the
//!   active read and the writes stay off it, and backup reads answer "unreadable" without
//!   spawning `security` (#496: a locked keychain, polled every 3 s, was a stream of unlock
//!   dialogs). The cooldown is monotonic, so a wall-clock jump can't shorten or stretch it.
//! - The unclaimed-credential stash: base64 0600 files under `<store>/credentials/`, outside
//!   the Keychain on purpose (`stash.rs`), and the refresh gate's rows in it, the successors
//!   it parks and adopts (`successor.rs`).
//! - The machine-shared fields of a credential (`shared.rs`) and the global config's
//!   read-modify-write (`global_config.rs`).
//!
//! Left out (`docs/claude-accounts.md` 3): the file backend. Where cswap sends a write to a
//! plaintext file because the Keychain is unusable (the active `.credentials.json` as a store,
//! the `.enc` backups and their `.enc`-wins reads and reconcile, the `.enc.prev` copies), Koyo
//! has no file to fall back to: while the Keychain is backed off, or when a write to it fails,
//! the write fails with [`crate::AccountsError::CredentialWrite`] and nothing lands in a file.
//! The file-mode pin goes with it (`_pin_file_mode`, `_lift_pin_if_oauth_file_gone`, the
//! residual verdict), since only a fallback write pins. Managed API keys (`_read_managed_key`,
//! `_write_managed_credentials`, `_clear_managed_key`, `approved_form`, the `Claude Code`
//! item) are left out with API-key accounts: an API key is refused rather than written. Windows
//! and Linux are left out.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::Env;
use crate::keychain::{self, KeychainError, Security};
use crate::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE};

mod active;
mod backup;
mod global_config;
mod shared;
mod stash;
mod successor;

pub use backup::{backup_username, prev_backup_username};
pub use shared::{
    ACCOUNT_CREDENTIAL_KEYS, SHARED_CREDENTIAL_KEYS, credential_object, looks_like_api_key,
    merge_shared_credential_fields, shared_credential_fields,
};
pub use stash::ManifestVerdict;
pub use successor::{StashMeta, StashReason, StashedSuccessor};

/// The active OAuth read's attempts: a locked or contended login Keychain can fail one
/// `security` call (just after wake, or under Claude Code's own statusline polling the item)
/// and answer the next. An I/O backoff between retries of an external tool, not a race.
pub const ACTIVE_READ_ATTEMPTS: u32 = 2;
pub const ACTIVE_READ_RETRY_DELAY: Duration = Duration::from_millis(300);
/// How long a Keychain failure keeps the Keychain off: far longer than one operation runs, so
/// one operation can't split between answers, and short enough that a transient `security`
/// timeout heals within a minute in a long-running app.
pub const KEYCHAIN_RECHECK_COOLDOWN_S: Duration = Duration::from_secs(60);
/// How long a display read of a backup may be served from the cache. Below the usage store's
/// fetch floor (180 s), so a fetch always re-reads after another process's rotation.
pub const BACKUP_READ_CACHE_TTL_S: Duration = Duration::from_secs(60);

/// What reading Claude Code's active credential found, cswap's `ActiveCredentials`.
#[derive(Clone, PartialEq, Eq)]
pub struct ActiveCredentials {
    /// The credential, `""` when none is stored, `None` when `.credentials.json` exists but
    /// couldn't be read.
    pub value: Option<String>,
    /// The OAuth Keychain read failed (locked, denied, timed out) and nothing covered it: the
    /// slot is unreadable, not empty, so the person isn't sent to log in again.
    pub keychain_unavailable: bool,
    /// The OAuth Keychain read failed, even when the file covered it: the bytes may be a
    /// superseded generation. They may be shown or adopted, but their refresh token must never
    /// be spent: a superseded one answers `invalid_grant`, a false dead-token strike.
    pub degraded: bool,
}

impl ActiveCredentials {
    fn new(value: Option<String>, keychain_unavailable: bool, degraded: bool) -> Self {
        ActiveCredentials {
            value,
            keychain_unavailable,
            degraded,
        }
    }

    /// What a usage pass needs of this read: cswap's `_active_keychain_unavailable` and
    /// `_active_read_unreadable` as one state, since both mean nothing usable was read and
    /// both show `keychain unavailable`, and `_active_read_degraded`.
    pub fn verdict(&self) -> ActiveVerdict {
        ActiveVerdict {
            unavailable: self.keychain_unavailable || self.value.is_none(),
            degraded: self.degraded,
        }
    }
}

/// The active read's verdict as a usage pass carries it ([`ActiveCredentials::verdict`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ActiveVerdict {
    /// Nothing usable was read: the Keychain failed and nothing covered it, or
    /// `.credentials.json` exists and couldn't be read. The slot is unreadable, not empty.
    pub unavailable: bool,
    /// The Keychain read failed even if the file covered it: the bytes may be a superseded
    /// generation, never to be written into a backup or spent.
    pub degraded: bool,
}

/// A clean verdict, as cswap's switcher assumes for a thread that never read:
/// `ActiveCredentials("", False, False)`.
impl Default for ActiveCredentials {
    fn default() -> Self {
        ActiveCredentials::new(Some(String::new()), false, false)
    }
}

/// The value's length, never the value.
impl std::fmt::Debug for ActiveCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveCredentials")
            .field(
                "value",
                &self
                    .value
                    .as_ref()
                    .map(|value| format!("<{} bytes>", value.len())),
            )
            .field("keychain_unavailable", &self.keychain_unavailable)
            .field("degraded", &self.degraded)
            .finish()
    }
}

/// Why a backup read found nothing to give: skipped inside the cooldown (cswap's
/// `KeychainBackedOff`, still an unreadable verdict), or the Keychain failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupReadError {
    BackedOff,
    Keychain(KeychainError),
}

impl std::fmt::Display for BackupReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackupReadError::BackedOff => f.write_str(
                "backup read skipped: the Keychain is backed off after a recent failure",
            ),
            BackupReadError::Keychain(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BackupReadError {}

/// `sequence.json`'s inode, modification time (seconds, nanoseconds) and size.
type Generation = (u64, i64, i64, u64);

/// What the store learns while the process runs; one lock, never held across a `security`
/// call.
struct State {
    /// `_keychain_usable_cache`: where operations go. `None` before any has run.
    usable: Option<bool>,
    /// `_keychain_disabled_until`: when the cooldown a failure started ends.
    disabled_until: Option<Instant>,
    /// `_keychain_op_failed`: an operation failed and none has succeeded since. An
    /// observation, unlike `usable`, which is a routing choice.
    op_failed: bool,
    /// `_active_read_failed`: the last active read couldn't reach the Keychain. Per item: a
    /// readable backup says nothing about the active item, so it can't clear this.
    active_read_failed: bool,
    /// Where the last active write landed: `"keychain"` (Koyo has no file mode).
    last_active_credentials_backend: Option<&'static str>,
    /// Display reads of backups: `(number, email)` → (value or `""`, when read).
    backup_read_cache: HashMap<(String, String), (String, Instant)>,
    /// Bumped by every drop, so a fill that raced one isn't stored.
    backup_read_cache_epoch: u64,
    /// The registry generation the cache was filled under.
    backup_read_cache_gen: Option<Generation>,
    /// The cooldown already announced in the log: one line per cooldown.
    backoff_announced_until: Option<Instant>,
    /// [`KEYCHAIN_RECHECK_COOLDOWN_S`] and [`BACKUP_READ_CACHE_TTL_S`], as tests patch them.
    cooldown: Duration,
    cache_ttl: Duration,
}

/// The active credential and the per-account backups, cswap's `CredentialStore`: one per
/// process, its Keychain state learned from real `security` calls. Shared across threads
/// behind an `Arc`.
pub struct CredentialStore {
    env: Env,
    state: Mutex<State>,
}

impl CredentialStore {
    pub fn new(env: Env) -> CredentialStore {
        CredentialStore {
            env,
            state: Mutex::new(State {
                usable: None,
                disabled_until: None,
                op_failed: false,
                active_read_failed: false,
                last_active_credentials_backend: None,
                backup_read_cache: HashMap::new(),
                backup_read_cache_epoch: 0,
                backup_read_cache_gen: None,
                backoff_announced_until: None,
                cooldown: KEYCHAIN_RECHECK_COOLDOWN_S,
                cache_ttl: BACKUP_READ_CACHE_TTL_S,
            }),
        }
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The account of Claude Code's Keychain items: the active credential's, and every
    /// profile's the capture reads.
    pub(crate) fn account_name(&self) -> String {
        keychain::keychain_account_name(self.env.vars.user.as_deref())
    }

    /// Runs a Keychain operation, learning from it. A failure marks the Keychain unusable and
    /// starts the cooldown; a success (a miss included) clears the failure and marks it usable,
    /// but only from unknown: within one cooldown a success never sends operations back to it.
    /// `item_exists` doesn't come through here: it answers `false` for a failure too.
    ///
    /// # Errors
    /// The operation's own.
    pub fn kc_call<T>(
        &self,
        op: impl FnOnce(&dyn Security) -> Result<T, KeychainError>,
    ) -> Result<T, KeychainError> {
        let result = op(&*self.env.security);
        let mut state = self.state();
        if result.is_err() {
            state.op_failed = true;
            state.usable = Some(false);
            state.disabled_until = Some(Instant::now() + state.cooldown);
        } else {
            // A success is the newer observation: the Keychain answers. Routing stays.
            state.op_failed = false;
            if state.usable.is_none() {
                state.usable = Some(true);
            }
        }
        result
    }

    /// Whether operations should go to the Keychain now: until one fails, and again once the
    /// cooldown that failure started has passed.
    pub fn use_keychain(&self) -> bool {
        let mut state = self.state();
        if state.usable == Some(false)
            && state
                .disabled_until
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            state.usable = None;
            state.disabled_until = None;
        }
        state.usable != Some(false)
    }

    /// The Keychain can't be asked, so an empty read proves nothing: an operation failed, has
    /// not succeeded since, and the cooldown hasn't passed. Asked through
    /// [`CredentialStore::use_keychain`], where the cooldown's re-probe lives.
    pub fn keychain_unreadable(&self) -> bool {
        if self.use_keychain() {
            return false;
        }
        self.state().op_failed
    }

    /// Where the last active write landed: `"keychain"`, or `None` before any.
    pub fn last_active_credentials_backend(&self) -> Option<&'static str> {
        self.state().last_active_credentials_backend
    }
}

/// What cswap's tests set by hand, for this crate's tests and the app's.
#[cfg(any(test, feature = "testing"))]
impl CredentialStore {
    /// Ends the cooldown now, as cswap's tests set `_keychain_disabled_until` to a second ago.
    pub fn lapse_keychain_cooldown(&self) {
        let now = Instant::now();
        self.state().disabled_until = Some(now.checked_sub(Duration::from_secs(1)).unwrap_or(now));
    }

    /// cswap's `_keychain_usable_cache`, set.
    pub fn set_keychain_usable_cache(&self, usable: Option<bool>) {
        self.state().usable = usable;
    }

    /// `KEYCHAIN_RECHECK_COOLDOWN_S`, patched.
    pub fn set_keychain_cooldown(&self, cooldown: Duration) {
        self.state().cooldown = cooldown;
    }

    /// `BACKUP_READ_CACHE_TTL_S`, patched.
    pub fn set_backup_read_cache_ttl(&self, ttl: Duration) {
        self.state().cache_ttl = ttl;
    }
}

/// Whether the active config home is the default profile's, by where it resolves rather than
/// by `CLAUDE_CONFIG_DIR` being set: the default profile named explicitly is still the default.
/// Compared as `paths::same_directory` does (resolved, as written when either can't be
/// resolved); cswap's `Path.resolve()` also resolves the existing part of a folder that isn't
/// there yet, which matters only when neither folder exists.
pub fn active_profile_is_default(env: &Env) -> bool {
    paths::same_directory(
        &paths::claude_config_home(env),
        &paths::default_claude_config_home(env),
    )
}

/// The Keychain services holding the active environment's OAuth credential, in the order to
/// try them, as Claude Code (2.1.220 `getMacOsKeychainStorageServiceName`) derives them:
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` when it's defined (empty is the default store), else
/// `CLAUDE_CONFIG_DIR`. One case gets two: `CLAUDE_CONFIG_DIR` naming the default profile,
/// where Claude Code would write the hashed item but a person who always used the default may
/// only have the unsuffixed one. A defined secure-storage folder gets no fallback: it names
/// the only store Claude Code reads, so a miss is a logged-out profile.
pub fn active_oauth_keychain_services(env: &Env) -> Vec<String> {
    if let Some(secure) = env.vars.securestorage_config_dir.as_deref() {
        return vec![if secure.is_empty() {
            CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned()
        } else {
            paths::keychain_service_name(secure)
        }];
    }
    let Some(config_dir) = env.vars.config_dir() else {
        return vec![CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned()];
    };
    let mut services = vec![paths::keychain_service_name(config_dir)];
    if active_profile_is_default(env) {
        services.push(CLAUDE_CODE_KEYCHAIN_SERVICE.to_owned());
    }
    services
}

#[cfg(test)]
mod tests;
