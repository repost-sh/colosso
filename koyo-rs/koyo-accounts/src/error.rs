//! What can go wrong, one variant per exception of claude-swap's `exceptions.py` (0fe22a8) that
//! the ported modules raise, so a caller can tell them apart as cswap's do. Each carries the
//! sentence cswap would print, never a token.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum AccountsError {
    /// `CredentialError`: a credential is missing, malformed or refused.
    Credential(String),
    /// `CredentialReadError`: a credential couldn't be read.
    CredentialRead(String),
    /// `CredentialWriteError`: a credential couldn't be written.
    CredentialWrite(String),
    /// `ConfigError`: a file of the store or Claude Code's config is unreadable or unwritable.
    Config(String),
    /// `SwitchError`: a switch was refused or failed (and was rolled back).
    Switch(String),
    /// `LockError`: Koyo's own lock stayed held.
    Lock(String),
    /// `ClaudeCodeLockTimeout`: one of Claude Code's locks stayed held past the wait. Nothing
    /// was changed; the operation is safe to retry.
    ClaudeCodeLockTimeout(String),
    /// `AccountNotFoundError`.
    AccountNotFound(String),
    /// `ValidationError`: an input out of range.
    Validation(String),
    /// A write a sandboxed run aimed outside its sandbox or at the person's real login
    /// (`guard.rs`). Never raised outside tests.
    Guarded(PathBuf),
    /// The file system, with what was being done.
    Io(String, std::io::Error),
}

impl AccountsError {
    pub fn io(doing: impl Into<String>, error: std::io::Error) -> AccountsError {
        AccountsError::Io(doing.into(), error)
    }
}

impl fmt::Display for AccountsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountsError::Credential(why)
            | AccountsError::CredentialRead(why)
            | AccountsError::CredentialWrite(why)
            | AccountsError::Config(why)
            | AccountsError::Switch(why)
            | AccountsError::Lock(why)
            | AccountsError::ClaudeCodeLockTimeout(why)
            | AccountsError::AccountNotFound(why)
            | AccountsError::Validation(why) => f.write_str(why),
            AccountsError::Guarded(path) => write!(
                f,
                "refused a write to {}: outside the sandbox or under the real login",
                path.display()
            ),
            AccountsError::Io(doing, error) => write!(f, "{doing}: {error}"),
        }
    }
}

impl std::error::Error for AccountsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AccountsError::Io(_, error) => Some(error),
            _ => None,
        }
    }
}

pub type Result<T, E = AccountsError> = std::result::Result<T, E>;
