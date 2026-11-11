//! Everything the accounts logic reads from the world, in one value: the home folder, Claude
//! Code's environment variables, Koyo's store, the `security` tool, HTTP and the clock.
//!
//! claude-swap reads these from its own process (`Path.home()`, `os.environ`, `time.time()`)
//! and its tests patch them; here they're handed in, so a test sets one root and nothing it
//! runs can reach the person's real login (`docs/claude-accounts.md` 7). The app builds the
//! system's with [`Env::system`]; tests build a sandbox (`testing::Sandbox`), whose every write
//! the guard (`guard.rs`) holds to the sandbox's folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::http::Http;
use crate::keychain::Security;

/// Claude Code's environment variables that decide where its login lives, as the Claude Code
/// processes on the Mac see them: the app passes its login shell's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vars {
    /// `CLAUDE_CONFIG_DIR`, unset or empty meaning the default profile.
    pub claude_config_dir: Option<String>,
    /// `CLAUDE_SECURESTORAGE_CONFIG_DIR`: defined but empty selects the default secure store.
    pub securestorage_config_dir: Option<String>,
    /// `$USER`, the Keychain item's account when set.
    pub user: Option<String>,
}

impl Vars {
    /// This process's own.
    pub fn from_process() -> Vars {
        Vars {
            claude_config_dir: std::env::var("CLAUDE_CONFIG_DIR").ok(),
            securestorage_config_dir: std::env::var("CLAUDE_SECURESTORAGE_CONFIG_DIR").ok(),
            user: std::env::var("USER").ok(),
        }
    }

    /// `CLAUDE_CONFIG_DIR` when set and non-empty, as cswap's `os.environ.get(...)` truthiness.
    pub fn config_dir(&self) -> Option<&str> {
        self.claude_config_dir
            .as_deref()
            .filter(|dir| !dir.is_empty())
    }
}

/// The time, in seconds since the epoch, as cswap's `time.time()`.
pub trait Clock: Send + Sync {
    fn now(&self) -> f64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0., |since| since.as_secs_f64())
    }
}

#[derive(Clone)]
pub struct Env {
    /// The home folder, cswap's `Path.home()`.
    pub home: PathBuf,
    pub vars: Vars,
    /// Koyo's store, cswap's backup root: `<data dir>/claude-accounts`.
    pub store: PathBuf,
    pub security: Arc<dyn Security>,
    pub http: Arc<dyn Http>,
    pub clock: Arc<dyn Clock>,
    /// Set for a sandbox: every write must land under it, and none under the real login.
    pub(crate) sandbox: Option<PathBuf>,
}

impl Env {
    /// The Mac's: `$HOME`, the given variables, `/usr/bin/security`, the system `curl` and the
    /// system clock, with the store at `store`.
    ///
    /// # Errors
    /// In this crate's own tests, always: they run on a sandbox, never on the real login.
    pub fn system(store: PathBuf, vars: Vars) -> crate::Result<Env> {
        if cfg!(test) {
            return Err(crate::AccountsError::Guarded(store));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .ok_or_else(|| crate::AccountsError::Config("$HOME isn't set to a folder".into()))?;
        Ok(Env {
            home,
            vars,
            store,
            security: Arc::new(crate::keychain::SystemSecurity::default()),
            http: Arc::new(crate::http::Curl),
            clock: Arc::new(SystemClock),
            sandbox: None,
        })
    }

    pub fn now(&self) -> f64 {
        self.clock.now()
    }

    /// The sandbox's folder, for a sandboxed run.
    pub fn sandbox(&self) -> Option<&Path> {
        self.sandbox.as_deref()
    }
}
