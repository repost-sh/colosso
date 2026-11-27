//! One try at an update: a folder of its own beside the bundle, and whether anyone still
//! holds it (`docs/updates.md` 3).

use std::ffi::OsStr;
use std::fs::{DirBuilder, File};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::blocking::Blocking;

/// The staged bundle's name in an attempt's folder, as the app is named at an image's root.
pub(super) const STAGED: &str = "Koyo.app";

/// One attempt's folder beside the bundle,
/// `<folder of the bundle>/.<Koyo.app>.update-<pid>-<nanos>`, readable by its owner alone, and
/// an exclusive lock on it, held for as long as the attempt lives. On the bundle's volume, so
/// the staged bundle and the installed one are exchanged in one step, and no `.app`, so Launch
/// Services never registers it. The folder is one attempt's alone, so two Koyos never share
/// one. The lock is what says the attempt is alive: the kernel lets go of it when its holder
/// ends, however that happens, so a folder nobody holds was abandoned, and [`sweep`] removes
/// it.
///
/// ```text
/// image.dmg     the download, removed once the app is copied out of it
/// image/        where the image is mounted meanwhile
/// Koyo.app      the staged bundle
/// ```
#[derive(Debug)]
pub(super) struct Attempt {
    folder: PathBuf,
    /// The folder, open and locked: nothing is read or written through it.
    lock: OwnedFd,
}

impl Attempt {
    /// Makes an attempt's folder beside `bundle` and takes its lock. It's the first thing an
    /// install writes, so a place Koyo can't write fails here, before anything downloads.
    pub(super) fn begin(bundle: &Path, _: Blocking<'_>) -> io::Result<Attempt> {
        let (beside, prefix) = place(bundle)
            .ok_or_else(|| io::Error::other("the bundle is in no folder"))?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH);
        let nanos = now.map_or(0, |since| since.as_nanos());
        let folder = beside.join(format!("{prefix}{}-{nanos}", std::process::id()));
        DirBuilder::new().mode(0o700).create(&folder)?;
        let lock = File::open(&folder)?;
        lock.lock()?;
        Ok(Attempt {
            folder,
            lock: lock.into(),
        })
    }

    /// The attempt of `bundle`'s in `folder`, once the Koyo that holds it has ended: waits
    /// for its lock. A folder that isn't beside `bundle` under an attempt's name is nobody's
    /// to hand to its installer, and is refused unopened.
    pub(super) fn once_let_go(bundle: &Path, folder: &Path, _: Blocking<'_>) -> io::Result<Attempt> {
        let beside = place(bundle).is_some_and(|(beside, prefix)| {
            folder.parent() == Some(beside) && named(folder.file_name(), &prefix)
        });
        if !beside {
            let said = format!("it isn't an attempt beside {}", bundle.display());
            return Err(io::Error::new(io::ErrorKind::InvalidInput, said));
        }
        let lock = File::open(folder)?;
        lock.lock()?;
        Ok(Attempt {
            folder: folder.to_owned(),
            lock: lock.into(),
        })
    }

    /// The attempt in `folder` if nobody holds it.
    fn abandoned(folder: PathBuf, _: Blocking<'_>) -> Option<Attempt> {
        let lock = File::open(&folder).ok()?;
        lock.try_lock().ok()?;
        Some(Attempt {
            folder,
            lock: lock.into(),
        })
    }

    pub(super) fn folder(&self) -> &Path {
        &self.folder
    }

    pub(super) fn image(&self) -> PathBuf {
        self.folder.join("image.dmg")
    }

    pub(super) fn mount(&self) -> PathBuf {
        self.folder.join("image")
    }

    pub(super) fn staged(&self) -> PathBuf {
        self.folder.join(STAGED)
    }

    /// Removes the folder and everything in it, an image still mounted in it detached first,
    /// while the lock is held: nothing else takes it for abandoned meanwhile.
    pub(super) fn discard(self, may: Blocking<'_>) {
        let mount = self.mount();
        if mounted(&mount, &self.folder, may)
            && let Err(error) = detach(&mount, may)
        {
            log::warn!("updates: {error}");
        }
        if let Err(error) = std::fs::remove_dir_all(&self.folder) {
            log::warn!("updates: couldn't remove {}: {error}", self.folder.display());
        }
    }

    /// Keeps the lock until this process ends: the attempt was handed to the installer, which
    /// waits on it, so nothing lets go of it but the kernel, once the process is gone.
    pub(super) fn hold_until_the_end(self) {
        std::mem::forget(self.lock);
    }
}

/// The folder `bundle` is in, and what the name of each of its attempts' folders starts with.
fn place(bundle: &Path) -> Option<(&Path, String)> {
    let name = bundle.file_name()?.to_string_lossy();
    Some((bundle.parent()?, format!(".{name}.update-")))
}

/// Whether `name`, a file's, is an attempt folder's: it starts with `prefix`.
fn named(name: Option<&OsStr>, prefix: &str) -> bool {
    name.is_some_and(|name| name.to_string_lossy().starts_with(prefix))
}

/// Removes the attempt folders beside `bundle` that nobody holds: what a Koyo that crashed or
/// quit mid-install left, and the bundle an installer replaced. A folder whose lock is held
/// belongs to a living Koyo or to an installer at work, and is left alone.
pub(super) fn sweep(bundle: &Path, may: Blocking<'_>) {
    let Some((beside, prefix)) = place(bundle) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(beside) else {
        return;
    };
    for entry in entries.flatten() {
        if !named(Some(&entry.file_name()), &prefix) {
            continue;
        }
        if let Some(attempt) = Attempt::abandoned(entry.path(), may) {
            log::info!("updates: removing {}, which nobody holds", attempt.folder.display());
            attempt.discard(may);
        }
    }
}

/// Whether a volume is mounted at `at`, a folder in `folder`: it's on another device then.
fn mounted(at: &Path, folder: &Path, _: Blocking<'_>) -> bool {
    match (std::fs::metadata(at), std::fs::metadata(folder)) {
        (Ok(at), Ok(folder)) => at.dev() != folder.dev(),
        _ => false,
    }
}

/// Unmounts the image mounted at `at`, whatever still reads it: it's read-only and nobody's
/// but the attempt's.
pub(super) fn detach(at: &Path, may: Blocking<'_>) -> Result<(), String> {
    let mut hdiutil = Command::new("/usr/bin/hdiutil");
    hdiutil.args(["detach", "-force"]).arg(at);
    run(&mut hdiutil, "hdiutil", may)
}

/// Runs `tool` to its end with nothing on its stdin: failed, what it said.
pub(super) fn run(tool: &mut Command, name: &str, _: Blocking<'_>) -> Result<(), String> {
    let output = tool
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("couldn't run {name}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    Err(match said.trim() {
        "" => format!("{name} failed: {}", output.status),
        said => said.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::{Attempt, sweep};
    use crate::blocking::in_test;
    use crate::test_support::scratch;
    use crate::updates::fixtures;

    #[test]
    fn an_attempt_has_a_folder_of_its_own_beside_the_bundle() {
        let beside = scratch("update-attempt");
        let bundle = beside.join("Koyo.app");
        in_test(|may| {
            let first = Attempt::begin(&bundle, may).unwrap();
            let second = Attempt::begin(&bundle, may).unwrap();
            assert_ne!(first.folder(), second.folder());
            for attempt in [&first, &second] {
                let folder = attempt.folder();
                assert_eq!(folder.parent(), Some(beside.as_path()));
                let name = folder.file_name().unwrap().to_string_lossy().into_owned();
                let begins = format!(".Koyo.app.update-{}-", std::process::id());
                assert!(name.starts_with(&begins), "{name}");
                let mode = std::fs::metadata(folder).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o700);
            }
            assert_eq!(first.staged(), first.folder().join("Koyo.app"));
            first.discard(may);
            second.discard(may);
        });
        assert_eq!(std::fs::read_dir(&beside).unwrap().count(), 0);
    }

    #[test]
    fn a_place_that_cannot_be_written_fails_before_anything_else() {
        let beside = scratch("update-attempt-read-only");
        let bundle = beside.join("Koyo.app");
        std::fs::set_permissions(&beside, std::fs::Permissions::from_mode(0o555)).unwrap();
        let begun = in_test(|may| Attempt::begin(&bundle, may));
        std::fs::set_permissions(&beside, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(begun.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_sweep_removes_what_nobody_holds_and_leaves_what_somebody_does() {
        let beside = scratch("update-sweep");
        let bundle = beside.join("Koyo.app");
        std::fs::create_dir(&bundle).unwrap();
        in_test(|may| {
            let held = Attempt::begin(&bundle, may).unwrap();
            let let_go = Attempt::begin(&bundle, may).unwrap();
            std::fs::create_dir_all(let_go.staged().join("Contents")).unwrap();
            std::fs::write(let_go.image(), b"half an image").unwrap();
            let abandoned = fixtures::let_go(let_go);
            let other = beside.join(".Other.app.update-1-1");
            std::fs::create_dir(&other).unwrap();

            sweep(&bundle, may);

            assert!(!abandoned.exists(), "an attempt nobody holds stayed");
            assert!(held.folder().is_dir(), "an attempt somebody holds went");
            assert!(bundle.is_dir() && other.is_dir(), "what isn't this bundle's attempt went");

            let folder = fixtures::let_go(held);
            sweep(&bundle, may);
            assert!(!folder.exists(), "an attempt let go since the last sweep stayed");
        });
    }
}
