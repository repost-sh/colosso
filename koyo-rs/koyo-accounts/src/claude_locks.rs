//! Port of claude-swap's `claude_locks.py` (0fe22a8): Claude Code's own advisory locks, taken
//! while Koyo changes its files.
//!
//! Claude Code guards its OAuth refresh with npm's `proper-lockfile`, and its `~/.claude.json`
//! writes the same way (verified against 2.1.218):
//!
//! - The lock is a **directory**; `mkdir`'s atomicity is the mutex.
//! - The refresh takes **two** locks, in order: `<config home>/.oauth_refresh.lock`, then the
//!   legacy `<config home>.lock` (`~/.claude.lock`). Both are stale only past **60 s**, and a
//!   live holder touches them every 5 s. On a contended legacy lock Claude Code releases the
//!   primary and retries.
//! - The config lock (`~/.claude.json.lock`) is stale after 10 s.
//! - Claude Code retries a held credentials lock 5 times with 1–2 s jittered sleeps, so
//!   holding it briefly is cooperative.
//!
//! Holding them closes the one real race with a running Claude Code: its refresh reads the
//! credential, refreshes over the network and saves under both locks, so a switch landing in
//! between would be overwritten by the old account's refreshed token. Under the lock, Claude
//! Code's own double-checked re-read sees the switched credential and skips its refresh.
//!
//! A stale lock is taken over under a sibling `.takeover` directory, re-checked inside it, so
//! two waiters that saw the same dead holder can't both remove it (#420). Claude Code never
//! looks at that directory.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::{AccountsError, Env, Result, fsutil, paths};

/// A credential lock younger than this belongs to a live holder and is never stolen: its
/// toucher may stall well past 10 s (suspend, a blocked event loop) while it still holds it.
pub const CREDENTIALS_STALENESS: Duration = Duration::from_secs(60);
/// The config lock keeps proper-lockfile's defaults: stale after 10 s.
pub const CONFIG_STALENESS: Duration = Duration::from_secs(10);
/// A little faster than Claude Code's 5 s, for margin.
pub const TOUCH_INTERVAL: Duration = Duration::from_secs(3);
/// Per lock: the credential locks are two taken one after the other, so ~2× at worst. Claude
/// Code holds one for a token round trip, or a local read-modify-write.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(9);
/// A short back-off after an arm declines, so the retry loop can't spin hot.
const DECLINE_BACKOFF: Duration = Duration::from_millis(50);
pub const TAKEOVER_SUFFIX: &str = ".takeover";
/// The takeover directory is held for one stat and rmdir; older, its holder died mid-takeover.
pub const TAKEOVER_STALENESS: Duration = Duration::from_secs(5);

/// The legacy credential lock, `~/.claude.lock`: Claude Code still takes it.
pub fn credentials_lock_dir(env: &Env) -> PathBuf {
    sibling_lock(&paths::claude_config_home(env))
}

/// Claude Code's primary OAuth refresh lock, `<config home>/.oauth_refresh.lock`.
pub fn oauth_refresh_lock_dir(env: &Env) -> PathBuf {
    paths::claude_config_home(env).join(".oauth_refresh.lock")
}

/// The global config's write lock, `~/.claude.json.lock`.
pub fn config_lock_dir(env: &Env) -> PathBuf {
    sibling_lock(&paths::global_config_path(env))
}

fn sibling_lock(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".lock");
    path.with_file_name(name)
}

/// How long ago `path` was last modified, as `time.time() - st_mtime`; `None` when it's gone.
fn age(path: &Path, follow: bool) -> Option<Duration> {
    let meta = if follow {
        path.metadata()
    } else {
        path.symlink_metadata()
    };
    let modified = meta.ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO),
    )
}

/// Sleeps at most what's left of the budget, never a whole draw past it.
fn nap(want: Duration, start: Instant, timeout: Duration) {
    let left = timeout.saturating_sub(start.elapsed());
    if !left.is_zero() {
        fsutil::nap(want.min(left));
    }
}

/// Removes `lock` if it's still stale, under the takeover directory; whether this call did.
fn remove_if_still_stale(env: &Env, lock: &Path, staleness: Duration) -> bool {
    let mut name = lock.file_name().unwrap_or_default().to_owned();
    name.push(TAKEOVER_SUFFIX);
    let takeover = lock.with_file_name(name);
    match fsutil::mkdir(env, &takeover) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if age(&takeover, false).is_some_and(|age| age > TAKEOVER_STALENESS) {
                // Its holder died mid-takeover; a file or a link isn't ours to keep.
                if fsutil::rmdir(env, &takeover).is_err() {
                    fsutil::remove_file(env, &takeover).ok();
                }
            }
            return false;
        }
        Err(_) => return false,
    }
    // Re-checked inside: the waiter before may already have removed the dead lock and taken
    // a fresh one at the same path.
    let removed =
        age(lock, true).is_some_and(|age| age > staleness) && fsutil::rmdir(env, lock).is_ok();
    fsutil::rmdir(env, &takeover).ok();
    removed
}

/// A held proper-lockfile lock, touched while it's held and removed when it's dropped.
pub struct LockDir {
    dir: PathBuf,
    env: Env,
    stop: Option<mpsc::Sender<()>>,
    toucher: Option<JoinHandle<()>>,
}

/// Takes the directory lock `dir` within `timeout` (default [`DEFAULT_TIMEOUT`]), taking over
/// one older than `staleness`, and keeps it fresh while it's held.
///
/// # Errors
/// [`AccountsError::ClaudeCodeLockTimeout`] when it stayed held; the guard's refusal.
pub fn proper_lockfile(
    env: &Env,
    dir: &Path,
    timeout: Option<Duration>,
    staleness: Duration,
) -> Result<LockDir> {
    let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
    if let Some(parent) = dir.parent() {
        fsutil::create_dir_all(env, parent)?;
    }
    let start = Instant::now();
    loop {
        match fsutil::mkdir(env, dir) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(AccountsError::io(
                    format!("taking {}", dir.display()),
                    error,
                ));
            }
        }
        if start.elapsed() > timeout {
            return Err(AccountsError::ClaudeCodeLockTimeout(format!(
                "Could not acquire {} — Claude Code appears to be refreshing credentials. \
                 Retry in a few seconds.",
                dir.file_name().unwrap_or_default().to_string_lossy()
            )));
        }
        let Some(held) = age(dir, true) else {
            // Back off here too: a dangling link at the lock's path answers "exists" to mkdir
            // and "missing" to stat, so this arm can repeat for the whole budget.
            nap(DECLINE_BACKOFF, start, timeout);
            continue;
        };
        if held > staleness {
            // A dead holder: removed and retaken. Declining must neither spin nor oversleep.
            if !remove_if_still_stale(env, dir, staleness) {
                nap(DECLINE_BACKOFF, start, timeout);
            }
            continue;
        }
        nap(jitter(), start, timeout);
    }
    let (stop, stopped) = mpsc::channel::<()>();
    let toucher = {
        let (env, dir) = (env.clone(), dir.to_owned());
        std::thread::spawn(move || touch_while_held(&env, &dir, &stopped, staleness))
    };
    Ok(LockDir {
        dir: dir.to_owned(),
        env: env.clone(),
        stop: Some(stop),
        toucher: Some(toucher),
    })
}

/// 0.25–0.5 s, as cswap's `0.25 + random() * 0.25`.
fn jitter() -> Duration {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    Duration::from_millis(250 + u64::from(nanos % 250))
}

fn touch_while_held(env: &Env, dir: &Path, stopped: &mpsc::Receiver<()>, staleness: Duration) {
    let mut warned = false;
    let mut last_ok = Instant::now();
    loop {
        match stopped.recv_timeout(TOUCH_INTERVAL) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
        match fsutil::touch(env, dir) {
            // The latch is per freeze, not per hold: a touch that lands ends the episode.
            Ok(()) => (last_ok, warned) = (Instant::now(), false),
            // Gone: nothing left to keep alive.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            // Transient, so it stays armed; said once the freeze outlives the staleness.
            Err(error) => {
                if !warned && last_ok.elapsed() > staleness {
                    warned = true;
                    log::warn!(
                        "Could not refresh {} ({error}); its mtime stops advancing, so a \
                         waiter may take it over as stale",
                        dir.display()
                    );
                }
            }
        }
    }
}

impl Drop for LockDir {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(toucher) = self.toucher.take() {
            toucher.join().ok();
        }
        match fsutil::rmdir(&self.env, &self.dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => log::warn!(
                "Lock {} vanished while held (taken over as stale?)",
                self.dir.display()
            ),
            Err(error) => log::warn!("Failed to release lock {}: {error}", self.dir.display()),
        }
    }
}

/// Both credential locks, held; dropped, the legacy goes first, then the primary, as a
/// `with` of the two unwinds.
pub struct CredentialsLock {
    _legacy: LockDir,
    _primary: LockDir,
}

/// Claude Code's credential-refresh locks, in its own order: the primary, then the legacy.
/// The pair and the order mean a waiting Koyo and a waiting Claude Code can't deadlock, and
/// exclusion holds after Claude Code drops the legacy lock. Both at its 60 s staleness.
///
/// # Errors
/// As [`proper_lockfile`]; the primary is released when the legacy times out.
pub fn claude_credentials_lock(env: &Env, timeout: Option<Duration>) -> Result<CredentialsLock> {
    let primary = proper_lockfile(
        env,
        &oauth_refresh_lock_dir(env),
        timeout,
        CREDENTIALS_STALENESS,
    )?;
    let legacy = proper_lockfile(
        env,
        &credentials_lock_dir(env),
        timeout,
        CREDENTIALS_STALENESS,
    )?;
    Ok(CredentialsLock {
        _legacy: legacy,
        _primary: primary,
    })
}

/// Claude Code's global-config write lock, `~/.claude.json.lock`.
///
/// # Errors
/// As [`proper_lockfile`].
pub fn claude_config_lock(env: &Env, timeout: Option<Duration>) -> Result<LockDir> {
    proper_lockfile(env, &config_lock_dir(env), timeout, CONFIG_STALENESS)
}

#[cfg(test)]
mod tests;
