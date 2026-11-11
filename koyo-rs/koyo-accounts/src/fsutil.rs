//! Port of claude-swap's `fsutil.py` (0fe22a8): the atomic writer, to its rules, and every
//! other write this crate makes, each asked of the guard (`guard.rs`) first. The Windows
//! retries, junctions and link probes are left out (`docs/claude-accounts.md` 3).
//!
//! A write publishes by temp file and rename: the temp is made beside the target, 0600 from
//! creation; the bytes are written, the mode set on the open file, the content fsynced, and the
//! temp renamed over the target. A reader sees the old file or the new one, never a torn one,
//! which Claude Code, watching `~/.claude.json`, takes for a corrupted config (#419). A symlink
//! is written through, never over: renaming onto the link would detach it (#192).

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::{AccountsError, Env, Result, guard};

/// What of the target's name the temp's carries: `NAME_MAX` is 255 bytes and the temp adds its
/// random part and `.tmp`, so a long legal name would otherwise overflow at the temp.
const TEMP_PREFIX_CHARS: usize = 40;
/// The most links followed to a write's target before it's taken for a loop.
const MAX_LINKS: usize = 40;

/// Where a write to `path` lands: `path`, or for a symlink the end of its chain, a missing
/// final referent allowed (a dangling link writes where it points), a loop refused.
///
/// # Errors
/// A chain longer than [`MAX_LINKS`], or one that can't be read.
pub fn publish_target(path: &Path) -> io::Result<PathBuf> {
    let mut target = path.to_owned();
    for _ in 0..MAX_LINKS {
        let meta = match fs::symlink_metadata(&target) {
            Ok(meta) => Some(meta),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            // A loop on the way (`loop/../victim.json`) is refused, not folded away.
            Err(error) => return Err(error),
        };
        match meta {
            Some(meta) if meta.file_type().is_symlink() => {
                let next = fs::read_link(&target)?;
                target = match target.parent() {
                    Some(parent) if next.is_relative() => parent.join(next),
                    _ => next,
                };
            }
            _ => {
                if target == path {
                    return Ok(target);
                }
                // As realpath: the folders on the way resolved too.
                let parent = target.parent().map(Path::canonicalize).transpose()?;
                return Ok(match (parent, target.file_name()) {
                    (Some(parent), Some(name)) => parent.join(name),
                    _ => target,
                });
            }
        }
    }
    Err(io::Error::from_raw_os_error(libc::ELOOP))
}

/// Publishes `data` at `path` (through a symlink there), 0600, as the module says.
///
/// # Errors
/// The guard's refusal, or the file system's. A failure before the rename leaves the
/// destination as it was and removes the temp.
pub fn atomic_write(env: &Env, path: &Path, data: &[u8]) -> Result<()> {
    atomic_write_mode(env, path, data, 0o600)
}

/// [`atomic_write`] with the file's `mode`.
///
/// # Errors
/// As [`atomic_write`].
pub fn atomic_write_mode(env: &Env, path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let target = publish_target(path)
        .map_err(|error| AccountsError::io(format!("resolving {}", path.display()), error))?;
    guard::check(env, &target)?;
    write_via_temp(&target, data, mode)
        .map_err(|error| AccountsError::io(format!("writing {}", target.display()), error))
}

fn write_via_temp(target: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let name: String = target
        .file_name()
        .map(|name| {
            name.to_string_lossy()
                .chars()
                .take(TEMP_PREFIX_CHARS)
                .collect()
        })
        .unwrap_or_default();
    let (temp, mut file) = loop {
        let temp = parent.join(format!("{name}.{}.tmp", random_suffix()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
        {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    };
    let written = (|| {
        file.write_all(data)?;
        fault(Step::Mode)?;
        if !skipped(Step::Mode) {
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        if let Err(error) = fault(Step::Fsync).and_then(|()| fsync(&file))
            && !error
                .raw_os_error()
                .is_some_and(|code| FSYNC_UNSUPPORTED.contains(&code))
        {
            return Err(error);
        }
        drop(file);
        fault(Step::Rename)?;
        fs::rename(&temp, target)
    })();
    if written.is_err() {
        fs::remove_file(&temp).ok();
    }
    written
}

/// The steps of a write a test can fail, as cswap's tests patch `os.fchmod`, `os.fsync` and
/// `replace_with_retry`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    Mode,
    Fsync,
    Rename,
}

#[cfg(test)]
thread_local! {
    /// The step this thread's next write fails at, with the error number it fails with.
    pub(crate) static FAULT: std::cell::Cell<Option<(Step, i32)>> = const { std::cell::Cell::new(None) };
}

/// The step fails with the error number set, or with 0 is skipped.
#[cfg(test)]
fn fault(step: Step) -> io::Result<()> {
    match FAULT.get() {
        Some((at, code)) if at == step && code != 0 => Err(io::Error::from_raw_os_error(code)),
        _ => Ok(()),
    }
}

#[cfg(test)]
fn skipped(step: Step) -> bool {
    FAULT.get() == Some((step, 0))
}

#[cfg(not(test))]
fn skipped(_: Step) -> bool {
    false
}

#[cfg(not(test))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the tests' seam, which fails in tests only"
)]
fn fault(_: Step) -> io::Result<()> {
    Ok(())
}

/// Filesystems that don't implement fsync answer one of these; the write goes ahead without
/// the durability (Claude Code's writer tolerates the same set). Anything else, EIO above all,
/// fails it.
const FSYNC_UNSUPPORTED: [i32; 4] = [libc::EINVAL, libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOSYS];

/// Eight characters no other temp beside the target has, as `mkstemp`'s.
fn random_suffix() -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let mut seed = u64::from(nanos)
        ^ (u64::from(std::process::id()) << 32)
        ^ COUNT
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (0..8)
        .map(|_| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            char::from(ALPHABET[(seed >> 33) as usize % ALPHABET.len()])
        })
        .collect()
}

/// cswap's `settings.atomic_write_json`: `data` as two-space JSON at `path`, its folder made if
/// need be and narrowed to 0700. The folder is `path`'s own, not a symlink target's: narrowing a
/// folder that belongs to something else would be wrong, and the file is 0600 either way.
///
/// # Errors
/// As [`atomic_write`], or the folder that couldn't be made.
pub fn atomic_write_json(env: &Env, path: &Path, data: &(impl Serialize + ?Sized)) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(env, parent)?;
        set_mode(env, parent, 0o700)
            .map_err(|error| AccountsError::io(format!("narrowing {}", parent.display()), error))?;
    }
    let text = serde_json::to_string_pretty(data)
        .map_err(|error| AccountsError::Config(format!("{}: {error}", path.display())))?;
    atomic_write(env, path, text.as_bytes())
}

/// `fs::create_dir_all`, asked of the guard.
///
/// # Errors
/// The guard's refusal, or the file system's.
pub fn create_dir_all(env: &Env, path: &Path) -> Result<()> {
    guard::check(env, path)?;
    fs::create_dir_all(path)
        .map_err(|error| AccountsError::io(format!("making {}", path.display()), error))
}

/// `mkdir`, asked of the guard, with the file system's answer as it is: a lock is taken by
/// it, and `AlreadyExists` is that lock held.
///
/// # Errors
/// The guard's refusal as `PermissionDenied`, or the file system's.
pub fn mkdir(env: &Env, path: &Path) -> io::Result<()> {
    guarded(env, path)?;
    fs::create_dir(path)
}

/// `rmdir`, asked of the guard.
///
/// # Errors
/// As [`mkdir`].
pub fn rmdir(env: &Env, path: &Path) -> io::Result<()> {
    guarded(env, path)?;
    fs::remove_dir(path)
}

/// `unlink`, asked of the guard.
///
/// # Errors
/// As [`mkdir`].
pub fn remove_file(env: &Env, path: &Path) -> io::Result<()> {
    guarded(env, path)?;
    fs::remove_file(path)
}

/// `rename`, asked of the guard for both paths: a corrupt file set aside.
///
/// # Errors
/// As [`mkdir`].
pub fn rename(env: &Env, from: &Path, to: &Path) -> io::Result<()> {
    guarded(env, from)?;
    guarded(env, to)?;
    fs::rename(from, to)
}

/// `chmod`, asked of the guard: cswap's 0700 on a folder of its store.
///
/// # Errors
/// As [`mkdir`].
pub fn set_mode(env: &Env, path: &Path, mode: u32) -> io::Result<()> {
    guarded(env, path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Sets `path`'s modification time to now, as `os.utime(path)`, asked of the guard.
///
/// # Errors
/// As [`mkdir`].
pub fn touch(env: &Env, path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt as _;
    guarded(env, path)?;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: `path` is a NUL-terminated string that outlives the call; a null `times` is
    // "now" for both, as `os.utime(path)`.
    if unsafe { libc::utimes(path.as_ptr(), std::ptr::null()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `os.fsync` as cswap calls it: on macOS a plain `fsync(2)`, which hands the data to the drive.
/// `File::sync_all` is `F_FULLFSYNC` there, which also flushes the drive's own cache and stalls
/// other I/O, on every write of the usage cache.
fn fsync(file: &fs::File) -> io::Result<()> {
    use std::os::fd::AsRawFd as _;
    // SAFETY: the descriptor is `file`'s, open for as long as it's borrowed.
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Opens `path` for writing, made 0600 if it's new, asked of the guard: a lock file.
///
/// # Errors
/// As [`mkdir`].
pub fn open_for_lock(env: &Env, path: &Path) -> io::Result<fs::File> {
    guarded(env, path)?;
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
}

fn guarded(env: &Env, path: &Path) -> io::Result<()> {
    guard::check(env, path).map_err(|error| io::Error::new(io::ErrorKind::PermissionDenied, error))
}

/// Waits `duration` on this thread: a lock's retry waits on the file system, which has no
/// event to wait on.
#[expect(
    clippy::disallowed_methods,
    reason = "a lock's retry polls the file system, which offers nothing to wait on"
)]
pub fn nap(duration: Duration) {
    std::thread::sleep(duration);
}

#[cfg(test)]
mod tests;
