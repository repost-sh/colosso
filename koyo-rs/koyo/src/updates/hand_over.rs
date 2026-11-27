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

/// Starts the installer and doesn't wait for it: spawned as it is, with no fork of the app
/// and no signal blocked, with nothing to read or write, and in a process group of its own,
/// so a hangup sent to the app's doesn't reach it.
fn start(exe: &Path, told: &HandOver, _: Blocking<'_>) -> io::Result<()> {
    let mut installer = Command::new(exe);
    installer
        .args(told.args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    koyo_daemon::with_no_signal_blocked(|| installer.spawn()).map(drop)
}

/// `koyo install-update`: waits for the Koyo that staged a release to end, checks the staged
/// bundle may replace the one this executable runs from, exchanges the two, removes the
/// attempt, the replaced bundle with it, and opens Koyo again if asked. Logs to Koyo's log. A
/// bundle that isn't replaced is as it was, and Koyo still opens again for a restart, so a
/// restart always brings a Koyo back.
pub fn main(args: impl Iterator<Item = String>, may: Blocking<'_>) -> ExitCode {
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
    let installed = bundle.display();
    let replaced = match Requirement::of_self(may) {
        Ok(trust) => {
            let itself = Itself { bundle: bundle.clone(), trust };
            install(&told.attempt, &itself, may).is_ok()
        }
        Err(why) => {
            log::error!("updates: {installed} wasn't replaced: its signature can't be read: {why}");
            false
        }
    };
    if told.reopen {
        match open(&bundle, may) {
            Ok(()) => log::info!("updates: opened {installed}"),
            Err(error) => log::error!("updates: couldn't open {installed}: {error}"),
        }
    }
    if replaced { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// The Koyo an installer is.
struct Itself {
    /// The bundle its executable runs from: the one thing it ever replaces.
    bundle: PathBuf,
    /// Its own designated requirement, which a staged bundle must satisfy.
    trust: Requirement,
}

/// Puts the release staged in the attempt `folder` in place, once the Koyo that holds it has
/// ended: the release now installed, or why the bundle is as it was, which it also says in
/// the log. The attempt is removed either way.
fn install(folder: &Path, itself: &Itself, may: Blocking<'_>) -> Result<Version, String> {
    let (shown, installed) = (folder.display(), itself.bundle.display());
    log::info!("updates: the installer waits for the Koyo that holds {shown} to end");
    let attempt = Attempt::once_let_go(&itself.bundle, folder, may);
    let replaced = match &attempt {
        Ok(attempt) => replace(attempt, itself, may),
        Err(error) => Err(format!("the installer couldn't take {shown}: {error}")),
    };
    match &replaced {
        Ok(version) => {
            log::info!("updates: {installed} is replaced with Koyo {version}, staged in {shown}");
        }
        Err(why) => log::error!("updates: {installed} wasn't replaced and is as it was: {why}"),
    }
    if let Ok(attempt) = attempt {
        attempt.discard(may);
    }
    replaced
}

/// Puts the bundle `attempt` holds in place of the installer's own, if it may replace it:
/// the release now installed, or why the installed bundle is as it was.
fn replace(attempt: &Attempt, itself: &Itself, may: Blocking<'_>) -> Result<Version, String> {
    let staged = attempt.staged();
    let holds = may_replace(&staged, &itself.trust, Version::macos(), may)
        .map_err(|failure| format!("the staged bundle may not replace it: {failure:?}"))?;
    exchange(&staged, &itself.bundle, may)
        .map_err(|error| format!("the two bundles couldn't be exchanged: {error}"))?;
    Ok(holds)
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
