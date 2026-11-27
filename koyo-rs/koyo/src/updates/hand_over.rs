//! Putting a staged release in place once the app has ended (`docs/updates.md` 3): `koyo
//! install-update`, this same executable, started as the app quits. It waits on the
//! attempt's lock, which the kernel lets go of when the Koyo that staged the release is gone,
//! and with it the browser engine, which starts its helpers from the bundle by path for as
//! long as it runs.
//!
//! Any process of this user can start the installer, so it trusts nothing it's told. It
//! replaces only the bundle its own executable runs from, takes only an attempt beside that
//! bundle, and holds the staged bundle to the checks the app made of it, itself, before it
//! exchanges anything (`staged::may_replace`). What remains: another process of this user can
//! still write into the attempt's folder between that check and the exchange, since nothing
//! a user's process makes is closed to that user's other processes.

use std::ffi::{CString, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use super::attempt::Attempt;
use super::feed::Release;
use super::installed::bundled;
use super::signature::Requirement;
use super::staged::{Staged, may_replace};
use super::version::Version;
use crate::blocking::Blocking;

/// The word the executable is run with to be the installer, `main.rs`'s to match.
pub const INSTALL_UPDATE: &str = "install-update";

/// What the app tells the installer: `koyo install-update --attempt <folder> [--reopen]`.
/// Written and read here alone: app and installer are one executable, so the two ends can't
/// be of two versions. The bundle to replace isn't told: it's the one the installer runs
/// from.
#[derive(Debug, PartialEq, Eq)]
struct HandOver {
    /// The attempt's folder, which holds the staged bundle.
    attempt: PathBuf,
    /// Whether Koyo opens again once it's replaced: a restart, not a quit.
    reopen: bool,
}

impl HandOver {
    fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            INSTALL_UPDATE.into(),
            "--attempt".into(),
            self.attempt.clone().into(),
        ];
        if self.reopen {
            args.push("--reopen".into());
        }
        args
    }

    /// What `args`, those after the mode's word, tell.
    fn parse(mut args: impl Iterator<Item = String>) -> Option<HandOver> {
        let (mut attempt, mut reopen) = (None, false);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--attempt" => attempt = Some(PathBuf::from(args.next()?)),
                "--reopen" => reopen = true,
                _ => return None,
            }
        }
        Some(HandOver {
            attempt: attempt?,
            reopen,
        })
    }
}

/// Hands `staged` to the installer, `exe` run as `koyo install-update`, which puts it in place
/// of the bundle `exe` runs from once this process has ended and, to `reopen`, opens Koyo
/// again. The attempt stays held until this process ends. An installer that didn't start
/// leaves nothing staged: the release it was comes back with why.
pub(super) fn hand_over(
    staged: Staged,
    exe: &Path,
    reopen: bool,
    may: Blocking<'_>,
) -> Result<(), (Release, String)> {
    let told = HandOver {
        attempt: staged.attempt().to_owned(),
        reopen,
    };
    match start(exe, &told, may) {
        Ok(()) => {
            log::info!(
                "updates: handed Koyo {} in {} to the installer{}",
                staged.release().version,
                told.attempt.display(),
                if reopen { ", to open it again" } else { "" }
            );
            staged.handed_over();
            Ok(())
        }
        Err(error) => {
            let release = staged.discard(may);
            Err((release, format!("Koyo couldn't start its installer: {error}")))
        }
    }
}

/// Starts the installer and doesn't wait for it: spawned as it is, with no fork of the app,
/// with nothing to read or write, and in a process group of its own, so a hangup sent to
/// the app's doesn't reach it.
fn start(exe: &Path, told: &HandOver, _: Blocking<'_>) -> io::Result<()> {
    Command::new(exe)
        .args(told.args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

/// `koyo install-update`: waits for the Koyo that staged a release to end, checks the staged
/// bundle may replace the one this executable runs from, exchanges the two, opens Koyo again
/// if asked, and removes the attempt, the replaced bundle with it. Logs to Koyo's log. A
/// bundle that isn't replaced is as it was, and Koyo still opens again for a restart, so a
/// restart always brings a Koyo back.
pub fn main(args: impl Iterator<Item = String>, may: Blocking<'_>) -> ExitCode {
    hear_signals();
    let Some(told) = HandOver::parse(args) else {
        log::error!("updates: {INSTALL_UPDATE} takes --attempt <folder> [--reopen]");
        return ExitCode::FAILURE;
    };
    let Some((_, bundle)) = bundled() else {
        log::error!(
            "updates: {INSTALL_UPDATE} isn't run from an app bundle, so it has no Koyo to replace"
        );
        return ExitCode::FAILURE;
    };
    let itself = Itself {
        bundle,
        trust: &Requirement::of_self,
        reopen: &open,
    };
    install(&told, &itself, may)
}

/// The Koyo an installer is, which a test stands in for.
struct Itself<'a> {
    /// The bundle its executable runs from: the one thing it ever replaces.
    bundle: PathBuf,
    /// Reads its own designated requirement, which a staged bundle must satisfy.
    trust: &'a dyn Fn(Blocking<'_>) -> Result<Requirement, String>,
    /// Opens the Koyo at a bundle.
    reopen: &'a dyn Fn(&Path, Blocking<'_>) -> io::Result<()>,
}

/// The installer, from its arguments on. Whatever keeps it from replacing its bundle, it
/// says why in the log, and for a restart opens the Koyo that's there.
fn install(told: &HandOver, itself: &Itself<'_>, may: Blocking<'_>) -> ExitCode {
    let (folder, installed) = (told.attempt.display(), itself.bundle.display());
    log::info!("updates: the installer waits for the Koyo that holds {folder} to end");
    let attempt = Attempt::once_let_go(&itself.bundle, &told.attempt, may);
    let replaced = match &attempt {
        Ok(attempt) => replace(attempt, itself, may),
        Err(error) => Err(format!("the installer couldn't take {folder}: {error}")),
    };
    match &replaced {
        Ok(version) => {
            log::info!("updates: {installed} is replaced with Koyo {version}, staged in {folder}");
        }
        Err(why) => log::error!("updates: {installed} wasn't replaced and is as it was: {why}"),
    }
    if told.reopen {
        match (itself.reopen)(&itself.bundle, may) {
            Ok(()) => log::info!("updates: opened {installed}"),
            Err(error) => log::error!("updates: couldn't open {installed}: {error}"),
        }
    }
    if let Ok(attempt) = attempt {
        attempt.discard(may);
    }
    if replaced.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Puts the bundle `attempt` holds in place of the installer's own, if it may replace it:
/// the release now installed, or why the installed bundle is as it was.
fn replace(attempt: &Attempt, itself: &Itself<'_>, may: Blocking<'_>) -> Result<Version, String> {
    let trust = (itself.trust)(may)
        .map_err(|why| format!("the installer's own signature can't be read: {why}"))?;
    let staged = attempt.staged();
    let holds = may_replace(&staged, &trust, Version::macos(), may)
        .map_err(|failure| format!("the staged bundle may not replace it: {failure:?}"))?;
    exchange(&staged, &itself.bundle, may)
        .map_err(|error| format!("the two bundles couldn't be exchanged: {error}"))?;
    Ok(holds)
}

/// Lets every signal through to this process. A child starts with the blocked signals of the
/// thread that spawned it, and the app's background threads, which start the installer for a
/// restart, block nearly all of them: left so, the installer could never be told to end.
fn hear_signals() {
    // SAFETY: the set is this function's own, initialised before it's read, and the call
    // changes this thread's blocked signals only.
    unsafe {
        let mut none = std::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&raw mut none);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const none, std::ptr::null_mut());
    }
}

/// Exchanges the bundles at `staged` and `installed` in one step: at every instant each path
/// holds one whole bundle or the other, and there is no moment with neither.
fn exchange(staged: &Path, installed: &Path, _: Blocking<'_>) -> io::Result<()> {
    let staged = CString::new(staged.as_os_str().as_bytes())?;
    let installed = CString::new(installed.as_os_str().as_bytes())?;
    // SAFETY: both are NUL-terminated strings that outlive the call.
    let exchanged =
        unsafe { libc::renamex_np(staged.as_ptr(), installed.as_ptr(), libc::RENAME_SWAP) };
    if exchanged == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Opens the Koyo at `bundle` through Launch Services, as a new instance: the one that ended
/// may not be gone from its list yet. Each `KOYO_*` variable this process inherited from the
/// app is passed on, so a restart keeps the data, config and log folders the app ran with.
fn open(bundle: &Path, _: Blocking<'_>) -> io::Result<()> {
    let mut open = Command::new("/usr/bin/open");
    open.arg("-n");
    for (name, value) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("KOYO_") {
            let mut variable = name;
            variable.push("=");
            variable.push(value);
            open.arg("--env").arg(variable);
        }
    }
    let ended = open.arg(bundle).stdin(Stdio::null()).status()?;
    if ended.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("open ended with {ended}")))
    }
}

#[cfg(test)]
mod tests;
