//! Port of claude-swap's `locking.py` (0fe22a8): Koyo's own cross-process lock, `flock` on a
//! file, waited for in 100 ms steps within a bound. The Windows branch isn't ported.

use std::fs::File;
use std::os::fd::AsRawFd as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{AccountsError, Env, Result, fsutil};

/// cswap's `FileLock` default: 10 s.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const STEP: Duration = Duration::from_millis(100);

/// A held lock; dropped, it's released.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    /// Takes the lock at `path` within `timeout`, making its folder if need be; `None` if it
    /// stayed held.
    ///
    /// # Errors
    /// The guard's refusal, or a lock file that can't be made.
    pub fn acquire(env: &Env, path: &Path, timeout: Duration) -> Result<Option<FileLock>> {
        if let Some(parent) = path.parent() {
            fsutil::create_dir_all(env, parent)?;
        }
        let file = fsutil::open_for_lock(env, path)
            .map_err(|error| AccountsError::io(format!("opening {}", path.display()), error))?;
        let start = Instant::now();
        loop {
            // SAFETY: `flock` on a descriptor this function owns.
            let taken =
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
            if taken {
                return Ok(Some(FileLock {
                    file,
                    path: path.to_owned(),
                }));
            }
            let spent = start.elapsed();
            if spent > timeout {
                return Ok(None);
            }
            // Clamped to what's left, so `timeout` bounds the call.
            fsutil::nap(STEP.min(timeout.saturating_sub(spent)));
        }
    }

    /// [`FileLock::acquire`], with a lock that stayed held an error, as cswap's `with
    /// FileLock(...)`.
    ///
    /// # Errors
    /// [`AccountsError::Lock`] when it stayed held; as [`FileLock::acquire`].
    pub fn hold(env: &Env, path: &Path, timeout: Duration) -> Result<FileLock> {
        FileLock::acquire(env, path, timeout)?.ok_or_else(|| {
            AccountsError::Lock("Failed to acquire lock - another instance may be running".into())
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // SAFETY: unlocking the descriptor this lock holds.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Sandbox;

    #[test]
    fn a_held_lock_times_out_within_its_bound_and_frees_on_drop() {
        let sandbox = Sandbox::new();
        let env = sandbox.env();
        let path = sandbox.store().join("nested/.lock");
        let held = FileLock::acquire(&env, &path, Duration::ZERO)
            .unwrap()
            .unwrap();
        let start = Instant::now();
        let second = FileLock::acquire(&env, &path, Duration::from_millis(250)).unwrap();
        assert!(second.is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            FileLock::hold(&env, &path, Duration::ZERO),
            Err(AccountsError::Lock(_))
        ));
        drop(held);
        assert!(
            FileLock::acquire(&env, &path, Duration::ZERO)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_zero_timeout_still_takes_a_free_lock() {
        let sandbox = Sandbox::new();
        let env = sandbox.env();
        let lock = FileLock::hold(&env, &sandbox.store().join(".lock"), Duration::ZERO).unwrap();
        assert!(lock.path().ends_with(".lock"));
    }
}
