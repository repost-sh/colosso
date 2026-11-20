//! What an agent gives a page, made safe to hand over: each path checked to be in its worktree's
//! checkout, and a folder's every entry too, then copied into a folder of Koyo's own that
//! nothing else writes, whose copies Chromium is given. Chromium reads a file it's given when
//! the page does, long after the check: the file itself could be a link out of the checkout by
//! then, and a copy can't. Each file is read through what was opened, checked to be a regular
//! file in the checkout, on the checkout's own volume (no other mounted inside it), no link and
//! no other name of a file elsewhere (a hard link). It's opened without waiting, so a pipe or a
//! device, which would wait for a writer, is refused at once.
//!
//! A copy is a clone of what was opened where the copies' folder is on its volume (APFS: no
//! room taken, the content the checked file's); elsewhere it's read, and one upload copies at
//! most [`MOST_BYTES`]. Chromium sets no bound on what a file input is given, which it reads from
//! disk as the page sends it, so the bound is Koyo's: 2 GiB, more than a page asks an agent for,
//! and a folder at most [`MOST_ENTRIES`] files, cloned or copied.
//!
//! The copies are this run's, in Koyo's data folder, each upload's in a folder made new for it:
//! what runs that ended left goes as the first upload is copied, and this run's as Koyo quits.

use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use koyo_app_control::ControlError;

use crate::blocking::Blocking;

/// The most one upload reads to copy, where it can't clone.
pub(super) const MOST_BYTES: u64 = 2 << 30;
/// The most files one folder upload gives.
pub(super) const MOST_ENTRIES: usize = 10_000;

/// What an upload may still take.
pub(super) struct Budget {
    /// Bytes read to copy, where the copies' folder is on another volume.
    pub bytes: u64,
    /// Files given.
    pub entries: usize,
}

impl Budget {
    pub(super) const fn whole() -> Budget {
        Budget {
            bytes: MOST_BYTES,
            entries: MOST_ENTRIES,
        }
    }
}

/// The checkout an upload is from: its folder, links followed, and the volume it's on.
struct Checkout<'a> {
    root: &'a Path,
    device: u64,
}

/// `paths`, absolute or relative to the checkout at `root`, each the file, or with `folder` the
/// folder, it is there, links followed, copied into `into`, a folder of Koyo's own: the copies'
/// paths, one folder of `into` each, under the name it had. Refused when one isn't there, is of
/// the other kind, is, or holds, anything outside the checkout, or is more than `budget`
/// allows; and `into` goes.
pub(super) fn given(
    root: &Path,
    paths: &[String],
    (folder, budget): (bool, Budget),
    into: &Path,
    may: Blocking<'_>,
) -> Result<Vec<String>, ControlError> {
    let copies = copied(root, paths, (folder, budget), into, may);
    if copies.is_err()
        && let Err(why) = fs::remove_dir_all(into)
    {
        log::warn!("an upload's copies, refused, {}: {why}", into.display());
    }
    copies
}

fn copied(
    root: &Path,
    paths: &[String],
    (folder, mut budget): (bool, Budget),
    into: &Path,
    may: Blocking<'_>,
) -> Result<Vec<String>, ControlError> {
    let root = fs::canonicalize(root).map_err(|why| {
        ControlError::not_found(format!(
            "Your worktree {} can't be read: {why}",
            root.display()
        ))
    })?;
    let device = fs::metadata(&root)
        .map_err(|why| not_copied(&root.display().to_string(), &why))?
        .dev();
    let checkout = Checkout {
        root: &root,
        device,
    };
    let mut copies = Vec::with_capacity(paths.len());
    for (at, path) in paths.iter().enumerate() {
        let real = inside(&root, path, folder, may)?;
        let name = real.file_name().unwrap_or(real.as_os_str());
        let copy = into.join(at.to_string()).join(name);
        made(copy.parent().unwrap_or(into), may).map_err(|why| not_copied(path, &why))?;
        if folder {
            copy_folder(&checkout, &real, &copy, &mut budget, may)?;
        } else {
            copy_file(&checkout, &real, &copy, &mut budget, may)?;
        }
        copies.push(copy.to_string_lossy().into_owned());
    }
    Ok(copies)
}

/// Where `path` is, links followed, given it's in the checkout at `root` and is a folder or a
/// file as `folder` says.
fn inside(root: &Path, path: &str, folder: bool, _: Blocking<'_>) -> Result<PathBuf, ControlError> {
    let given = PathBuf::from(path);
    let full = if given.is_absolute() {
        given
    } else {
        root.join(given)
    };
    let real = fs::canonicalize(&full).map_err(|why| {
        ControlError::not_found(format!("Nothing is at {}: {why}", full.display()))
    })?;
    if !real.starts_with(root) {
        return Err(outside(path, root));
    }
    if real.is_dir() != folder {
        let what = if folder {
            "is no folder"
        } else {
            "is a folder, not a file"
        };
        return Err(ControlError::invalid_input(format!("{path} {what}.")));
    }
    Ok(real)
}

/// The folder at `real` in `checkout`, copied to `to`, entry by entry. A link in it is
/// followed: one to a file in the checkout is copied as that file, one to a folder is left
/// out, as are one that leads nowhere and what's neither a file nor a folder, and one out of
/// the checkout refuses it all.
fn copy_folder(
    checkout: &Checkout<'_>,
    real: &Path,
    to: &Path,
    budget: &mut Budget,
    may: Blocking<'_>,
) -> Result<(), ControlError> {
    let shown = real.display().to_string();
    made(to, may).map_err(|why| not_copied(&shown, &why))?;
    let entries = fs::read_dir(real).map_err(|why| not_copied(&shown, &why))?;
    for entry in entries {
        let entry = entry.map_err(|why| not_copied(&shown, &why))?;
        let (from, copy) = (entry.path(), to.join(entry.file_name()));
        let kind = fs::symlink_metadata(&from)
            .map_err(|why| not_copied(&from.display().to_string(), &why))?
            .file_type();
        if kind.is_dir() {
            copy_folder(checkout, &from, &copy, budget, may)?;
        } else if kind.is_file() {
            copy_file(checkout, &from, &copy, budget, may)?;
        } else if kind.is_symlink() {
            let Ok(linked) = fs::canonicalize(&from) else {
                continue;
            };
            if !linked.starts_with(checkout.root) {
                return Err(outside(&from.display().to_string(), checkout.root));
            }
            if linked.is_file() {
                copy_file(checkout, &linked, &copy, budget, may)?;
            }
        }
    }
    Ok(())
}

/// The file at `real` in `checkout`, copied to `to`: opened without waiting, and cloned or read
/// through what was opened once that's known to be a regular file of the checkout's, on its
/// volume, and no other's.
fn copy_file(
    checkout: &Checkout<'_>,
    real: &Path,
    to: &Path,
    budget: &mut Budget,
    may: Blocking<'_>,
) -> Result<(), ControlError> {
    let shown = real.display().to_string();
    if budget.entries == 0 {
        return Err(ControlError::invalid_input(format!(
            "{shown} is one file more than browser_upload gives in one go, {MOST_ENTRIES}."
        )));
    }
    budget.entries -= 1;
    let file = (OpenOptions::new().read(true))
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(real)
        .map_err(|why| not_copied(&shown, &why))?;
    let about = file.metadata().map_err(|why| not_copied(&shown, &why))?;
    if !about.is_file() {
        return Err(ControlError::invalid_input(format!(
            "{shown} is no regular file: browser_upload gives a page files only, not a pipe or a device."
        )));
    }
    let opened = path_of(&file, may).map_err(|why| not_copied(&shown, &why))?;
    if !opened.starts_with(checkout.root) {
        return Err(outside(&shown, checkout.root));
    }
    if about.dev() != checkout.device {
        return Err(ControlError::invalid_input(format!(
            "{shown} is on another volume, mounted inside your worktree: browser_upload gives a page only files of your worktree's own."
        )));
    }
    if about.nlink() > 1 {
        return Err(ControlError::invalid_input(format!(
            "{shown} is also a file elsewhere, a hard link: browser_upload gives a page only files of your worktree's own."
        )));
    }
    if cloned(&file, to, may).map_err(|why| not_copied(&shown, &why))? {
        return Ok(());
    }
    read_into(file, to, &shown, budget, may)
}

/// `file` cloned to `to`, the person's only: `false` where the file system can't, as across
/// volumes.
fn cloned(file: &File, to: &Path, _: Blocking<'_>) -> io::Result<bool> {
    let name = CString::new(to.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: `fclonefileat` reads the descriptor, open while `file` is borrowed, and the path,
    // a string ended by a zero that lives through the call, and makes a new file there.
    let done = unsafe { libc::fclonefileat(file.as_raw_fd(), libc::AT_FDCWD, name.as_ptr(), 0) };
    if done == 0 {
        fs::set_permissions(to, Permissions::from_mode(0o600))?;
        return Ok(true);
    }
    let failed = io::Error::last_os_error();
    match failed.raw_os_error() {
        Some(libc::EXDEV | libc::ENOTSUP) => Ok(false),
        _ => Err(failed),
    }
}

/// `file` read into a new file at `to`, the person's only, within what `budget` has left.
fn read_into(file: File, to: &Path, shown: &str, budget: &mut Budget, _: Blocking<'_>) -> Result<(), ControlError> {
    let mut copy = (OpenOptions::new().write(true).create_new(true))
        .mode(0o600)
        .open(to)
        .map_err(|why| not_copied(shown, &why))?;
    let mut bounded = file.take(budget.bytes.saturating_add(1));
    let read = io::copy(&mut bounded, &mut copy).map_err(|why| not_copied(shown, &why))?;
    if read > budget.bytes {
        return Err(ControlError::invalid_input(format!(
            "{shown} takes the upload past {} GiB, the most browser_upload copies in one go from a worktree on another volume than Koyo's data.",
            MOST_BYTES >> 30
        )));
    }
    budget.bytes -= read;
    Ok(())
}

/// The path of what `file` opened, as the file system has it now.
fn path_of(file: &File, _: Blocking<'_>) -> io::Result<PathBuf> {
    let mut path = vec![0_u8; usize::try_from(libc::PATH_MAX).unwrap_or(1024)];
    // SAFETY: `F_GETPATH` writes at most `PATH_MAX` bytes, the path ended by a zero, into the
    // buffer it's given, which has that many; the descriptor is the open file's.
    let done = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, path.as_mut_ptr()) };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    let end = path.iter().position(|byte| *byte == 0).unwrap_or(path.len());
    path.truncate(end);
    Ok(PathBuf::from(OsString::from_vec(path)))
}

/// Makes `folder`, and those it's in, the person's only.
fn made(folder: &Path, _: Blocking<'_>) -> io::Result<()> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)
}

/// Where this run's copies are kept, in Koyo's data folder `data`.
fn this_runs(data: &Path) -> PathBuf {
    data.join("browser")
        .join("uploads")
        .join(std::process::id().to_string())
}

/// A folder of this run's for one upload's copies, in Koyo's data folder `data`, made new for
/// it, never one that was there; the first takes what runs that ended left first.
pub(super) fn folder_for_upload(data: &Path, may: Blocking<'_>) -> Result<PathBuf, ControlError> {
    static UPLOADS: AtomicU64 = AtomicU64::new(0);
    let run = this_runs(data);
    let failed = |folder: &Path, why: &io::Error| {
        ControlError::internal(format!(
            "Koyo couldn't make a folder for the files at {}: {why}",
            folder.display()
        ))
    };
    if UPLOADS.load(Ordering::Relaxed) == 0
        && let Some(uploads) = run.parent()
    {
        forget_ended(uploads, may);
    }
    made(&run, may).map_err(|why| failed(&run, &why))?;
    loop {
        let folder = run.join(UPLOADS.fetch_add(1, Ordering::Relaxed).to_string());
        match fs::DirBuilder::new().mode(0o700).create(&folder) {
            Ok(()) => return Ok(folder),
            Err(why) if why.kind() == io::ErrorKind::AlreadyExists => {}
            Err(why) => return Err(failed(&folder, &why)),
        }
    }
}

/// Takes away the copies in `uploads` of every run that ended: a folder named for a process that
/// no longer runs. A run still going keeps its own.
fn forget_ended(uploads: &Path, _: Blocking<'_>) {
    let Ok(runs) = fs::read_dir(uploads) else {
        return;
    };
    for left in runs.flatten() {
        let pid = left.file_name().to_str().and_then(|name| name.parse::<libc::pid_t>().ok());
        if pid.is_some_and(ended)
            && let Err(why) = fs::remove_dir_all(left.path())
        {
            log::warn!("an ended run's uploads, {}: {why}", left.path().display());
        }
    }
}

/// Whether no process `pid` runs.
fn ended(pid: libc::pid_t) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: `kill` with no signal only asks whether the process exists; it borrows nothing.
    let asked = unsafe { libc::kill(pid, 0) };
    asked == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Takes this run's copies away, in Koyo's data folder `data`, as Koyo quits.
pub(super) fn forget(data: &Path, _: Blocking<'_>) {
    let run = this_runs(data);
    if let Err(why) = fs::remove_dir_all(&run)
        && why.kind() != io::ErrorKind::NotFound
    {
        log::warn!("this run's uploads, {}: {why}", run.display());
    }
}

fn outside(path: &str, root: &Path) -> ControlError {
    ControlError::invalid_input(format!(
        "{path} is outside your worktree {}: browser_upload gives a page only files from it.",
        root.display()
    ))
}

fn not_copied(path: &str, why: &io::Error) -> ControlError {
    ControlError::internal(format!("{path} couldn't be copied for the page: {why}"))
}

#[cfg(test)]
mod tests;
