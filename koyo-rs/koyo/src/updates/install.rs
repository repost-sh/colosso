//! Turning a release into a checked copy beside the bundle, on a thread of its own
//! (`docs/updates.md` 3, 4).

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{Context, Task};

use super::Updates;
use super::attempt::{Attempt, STAGED, detach, run as run_tool};
use super::feed::Release;
use super::fetch::Fetch;
use super::installed::Installed;
use super::signature::{Requirement, Vouched};
use super::stage::Failure;
use super::staged::Staged;
use super::version::Version;
use crate::blocking::{self, Blocking};

/// What the job tells the entity.
#[derive(Debug)]
pub(super) enum Progress {
    /// So much of the image has come.
    Received(u64),
    /// The image is here: it's checked, and the app copied out of it and checked.
    Preparing,
    Done(Result<Staged, Failure>),
}

/// An install under way on a thread of its own, the one owner of the curl, hdiutil and ditto
/// it runs and of its attempt's folder. Dropping the `Job` closes `stop`, which the thread
/// notices as the next of the download arrives and between steps otherwise; it then removes
/// its attempt and ends. Nothing waits for it: as the app quits an install under way is
/// abandoned, its curl ends with the app, and the next launch removes the folder nobody holds.
pub(super) struct Job {
    _stop: Sender<()>,
    /// Hands each `Progress` to `Updates::moved`.
    _hearing: Task<()>,
}

impl Job {
    /// Starts installing `release`.
    pub(super) fn start(
        release: Release,
        installed: &Installed,
        fetch: Arc<dyn Fetch>,
        cx: &mut Context<Updates>,
    ) -> Result<Job, Failure> {
        let (stop, stopped) = std::sync::mpsc::channel();
        let (say, mut said) = unbounded();
        let work = Work {
            image: installed.feed.image(release.version),
            release,
            bundle: installed.bundle.clone(),
            trust: installed.trust.clone(),
            macos: installed.macos,
            fetch,
        };
        blocking::thread("koyo-update", move |may| {
            let tell = |progress| drop(say.unbounded_send(progress));
            match run(&work, &tell, &stopped, may) {
                Ok(staged) => tell(Progress::Done(Ok(staged))),
                Err(Halt::Failed(failure)) => {
                    log::warn!(
                        "updates: Koyo {} wasn't staged: {failure:?}",
                        work.release.version
                    );
                    tell(Progress::Done(Err(failure)));
                }
                Err(Halt::Stopped) => {}
            }
        })
        .map_err(|error| Failure::Broken(format!("The update's thread didn't start: {error}")))?;
        let hearing = cx.spawn(async move |updates, cx| {
            while let Some(progress) = said.next().await {
                if (updates.update(cx, |updates, cx| updates.moved(progress, cx))).is_err() {
                    return;
                }
            }
        });
        Ok(Job {
            _stop: stop,
            _hearing: hearing,
        })
    }
}

/// What the job's thread is given: all owned, all `Send`.
pub(super) struct Work {
    pub(super) release: Release,
    /// Where the release's image is.
    pub(super) image: String,
    /// The bundle to replace.
    pub(super) bundle: PathBuf,
    pub(super) trust: Requirement,
    /// This Mac's macOS.
    pub(super) macos: Version,
    pub(super) fetch: Arc<dyn Fetch>,
}

/// Why the job ended without a staged release.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Halt {
    /// The job was dropped: nobody waits for what it would say.
    Stopped,
    Failed(Failure),
}

impl From<Failure> for Halt {
    fn from(failure: Failure) -> Self {
        Halt::Failed(failure)
    }
}

/// Whether the job was dropped, which closed `stop`.
fn stopped(stop: &Receiver<()>) -> Result<(), Halt> {
    match stop.try_recv() {
        Err(TryRecvError::Disconnected) => Err(Halt::Stopped),
        Ok(()) | Err(TryRecvError::Empty) => Ok(()),
    }
}

/// The job, start to end: an attempt's folder beside the bundle, the image downloaded into
/// it, vouched for and the app copied out of it, and the copy checked. Ended any other way,
/// the attempt is gone again.
pub(super) fn run(
    work: &Work,
    said: &dyn Fn(Progress),
    stop: &Receiver<()>,
    may: Blocking<'_>,
) -> Result<Staged, Halt> {
    let attempt = Attempt::begin(&work.bundle, may).map_err(|error| unplaced(&work.bundle, &error))?;
    let version = work.release.version;
    log::info!(
        "updates: downloading Koyo {version} from {} into {}",
        work.image,
        attempt.folder().display()
    );
    if let Err(halt) = copy_in(work, &attempt, said, stop, may) {
        attempt.discard(may);
        return Err(halt);
    }
    let folder = attempt.folder().to_owned();
    let staged = Staged::check(attempt, &work.release, &work.trust, work.macos, may)?;
    log::info!("updates: Koyo {version} is staged in {}", folder.display());
    Ok(staged)
}

/// Why no attempt could begin beside `bundle`: a folder that can't be written is the
/// person's to change, by moving Koyo.
fn unplaced(bundle: &Path, error: &io::Error) -> Failure {
    let folder = bundle.parent().unwrap_or(bundle).to_owned();
    match error.kind() {
        io::ErrorKind::ReadOnlyFilesystem | io::ErrorKind::PermissionDenied => {
            Failure::ReadOnly { folder }
        }
        _ => Failure::Broken(format!(
            "Koyo couldn't make a folder in {}: {error}",
            folder.display()
        )),
    }
}

/// Downloads the image into `attempt`, checks it's signed as this Koyo is, and copies the
/// app out of it and onto the disk; the image is removed once the app is out.
fn copy_in(
    work: &Work,
    attempt: &Attempt,
    said: &dyn Fn(Progress),
    stop: &Receiver<()>,
    may: Blocking<'_>,
) -> Result<(), Halt> {
    download(work, &attempt.image(), said, stop, may)?;
    said(Progress::Preparing);
    let vouched = work.trust.vouch(attempt.image(), may)?;
    log::info!("updates: the image is signed as this Koyo is");
    stopped(stop)?;
    copy_out(&vouched, attempt, may).map_err(Failure::Broken)?;
    flush(&attempt.staged(), may)
        .map_err(|error| Failure::Broken(format!("The copy couldn't be written to disk: {error}")))?;
    if let Err(error) = std::fs::remove_file(vouched.image()) {
        log::warn!("updates: couldn't remove the image once the app was out: {error}");
    }
    stopped(stop)
}

/// Streams the release's image into the new file `to`, saying how much has come at most once
/// per percent.
fn download(
    work: &Work,
    to: &Path,
    said: &dyn Fn(Progress),
    stop: &Receiver<()>,
    may: Blocking<'_>,
) -> Result<(), Halt> {
    let into = File::create(to)
        .map_err(|error| Failure::Broken(format!("The download couldn't be kept: {error}")))?;
    let mut counting = Counting {
        into,
        received: 0,
        percent: 0,
        of: work.release.bytes,
        said,
        stop,
    };
    let streamed = work.fetch.stream(&work.image, work.release.bytes, &mut counting, may);
    stopped(stop)?;
    streamed.map_err(|why| Halt::Failed(Failure::Download(why)))
}

/// The download on its way into its file: counts what's written, and refuses the next write
/// once the job was asked to stop, which ends curl.
struct Counting<'a, W> {
    into: W,
    received: u64,
    /// The last percent said.
    percent: u64,
    /// How much is to come.
    of: u64,
    said: &'a dyn Fn(Progress),
    stop: &'a Receiver<()>,
}

impl<W: Write> Write for Counting<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if stopped(self.stop).is_err() {
            return Err(io::Error::other("the update was stopped"));
        }
        self.into.write_all(bytes)?;
        self.received += bytes.len() as u64;
        let percent = (self.received * 100).checked_div(self.of).unwrap_or(100);
        if percent > self.percent {
            self.percent = percent;
            (self.said)(Progress::Received(self.received));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.into.flush()
    }
}

/// Copies the app at the root of the `vouched` image into `attempt`, the image mounted in
/// the attempt's folder meanwhile, out of Finder's sight, and unmounted after, however the
/// copy went.
fn copy_out(vouched: &Vouched, attempt: &Attempt, may: Blocking<'_>) -> Result<(), String> {
    let mount = attempt.mount();
    std::fs::create_dir(&mount).map_err(|error| format!("The image has nowhere to open: {error}"))?;
    let mut attach = Command::new("/usr/bin/hdiutil");
    attach
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mount)
        .arg(vouched.image());
    run_tool(&mut attach, "hdiutil", may)?;
    let mut ditto = Command::new("/usr/bin/ditto");
    ditto.arg(mount.join(STAGED)).arg(attempt.staged());
    let copied = run_tool(&mut ditto, "ditto", may);
    let detached = detach(&mount, may);
    copied.and(detached)?;
    std::fs::remove_dir(&mount).map_err(|error| format!("The image didn't close: {error}"))
}

/// Has the drive write the copy at `copy`: every file in it, then every folder, each after
/// those in it, so a folder's entries name what is already written. `ditto` ends before what
/// it copied is on the disk, and a restart exchanges the bundle seconds later: a power loss
/// just after would leave the installed path holding files with nothing in them. A link is
/// its folder's entry and holds nothing of its own.
fn flush(copy: &Path, _: Blocking<'_>) -> io::Result<()> {
    let mut folders = vec![copy.to_owned()];
    let mut listed = 0;
    while let Some(folder) = folders.get(listed).cloned() {
        listed += 1;
        for entry in std::fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(entry.path());
            } else if kind.is_file() {
                File::open(entry.path())?.sync_all()?;
            }
        }
    }
    for folder in folders.iter().rev() {
        File::open(folder)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
