//! The settings file, watched for an edit made outside the app, in an editor beside it say, on
//! a thread of its own: a kqueue waits on the file, which an edit in place changes, as does a
//! save that renames a new file over it (the file watched goes, and the new one is watched
//! from then on, checked to be the one the path names once its watch is set). Its folder is
//! watched only while the file can't be opened, there being none or it being unreadable, for
//! that to change, so the app's other files there, written as the app runs, don't wake it.
//! Once what woke it settles, the thread asks when the file last changed, and only when that
//! moved reads it and hands it to the main thread. Nothing wakes while the file is left alone,
//! and an edit lands whether Koyo is in front or not. A watch that fails is said and set again
//! a while later, longer each time, so it neither spins nor stops for good. `FSEvents` isn't
//! used: it would wake for every write in the folder's tree, the browser's profiles among them.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use futures::StreamExt as _;
use gpui_kit::App;
use kqueue::{EventData, EventFilter, FilterFlag, Watcher};

use super::file::{self, Looked};
use crate::blocking::{self, Blocking};

/// How long the file and its folder must be still after a change before the file is looked
/// at: a save is several writes.
const SETTLE: Duration = Duration::from_millis(100);
/// How long after a watch failed it's set again, the first time, and at most: it doubles each
/// time it fails again.
const AGAIN_FIRST: Duration = Duration::from_secs(1);
const AGAIN_LAST: Duration = Duration::from_secs(60);
/// How many times a watch is set on a file that isn't the one the path names by the time its
/// watch is set, a save renaming another over it each time, before the watch counts as failed.
const TRIES: usize = 3;

/// Watches the file at `path`, last changed at `seen` as the app read it, taking up each edit
/// made outside the app.
pub(super) fn start(path: PathBuf, seen: Option<SystemTime>, cx: &App) {
    let (tell, mut told) = futures::channel::mpsc::unbounded();
    let started = blocking::thread("settings watch", move |may| {
        watch(&path, seen, may, |looked| tell.unbounded_send(looked).is_ok());
    });
    if let Err(error) = started {
        log::warn!("couldn't watch the settings file: {error}");
        return;
    }
    cx.spawn(async move |cx| {
        while let Some(looked) = told.next().await {
            cx.update(|cx| file::take_up(looked, cx));
        }
    })
    .detach();
}

/// Waits on the file at `path`, telling `tell` what it holds each time it changed after
/// `seen`, until `tell` answers that nobody listens. A watch that fails is set again.
pub(super) fn watch(
    path: &Path,
    seen: Option<SystemTime>,
    may: Blocking<'_>,
    tell: impl FnMut(Looked) -> bool,
) {
    watch_opening(path, seen, may, tell, |path| File::open(path));
}

/// [`watch`], opening the file to watch with `open`.
fn watch_opening(
    path: &Path,
    mut seen: Option<SystemTime>,
    may: Blocking<'_>,
    mut tell: impl FnMut(Looked) -> bool,
    mut open: impl FnMut(&Path) -> io::Result<File>,
) {
    let (_never, wait) = std::sync::mpsc::channel::<()>();
    let mut again = AGAIN_FIRST;
    loop {
        let Err(error) = follow(path, &mut seen, may, &mut tell, &mut open) else {
            return;
        };
        log::warn!(
            "the watch on {} failed, and is set again in {} s: {error}",
            path.display(),
            again.as_secs()
        );
        let _ = wait.recv_timeout(again);
        again = (again * 2).min(AGAIN_LAST);
    }
}

/// One watch on the file at `path`, until it fails or `tell` answers that nobody listens.
fn follow(
    path: &Path,
    seen: &mut Option<SystemTime>,
    may: Blocking<'_>,
    tell: &mut impl FnMut(Looked) -> bool,
    open: &mut impl FnMut(&Path) -> io::Result<File>,
) -> io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(folder)?;
    let mut watcher = Watcher::new()?;
    let mut held = None;
    loop {
        let unopened = rewatch(&mut watcher, &mut held, path, folder, open, may)?;
        let modified = file::modified(path, may);
        if modified != *seen {
            *seen = modified;
            if !tell(file::look(path, modified, may)) {
                return Ok(());
            }
        }
        let came_meanwhile = unopened == Some(io::ErrorKind::NotFound) && modified.is_some();
        if !came_meanwhile {
            settled(&watcher)?;
        }
    }
}

/// Watches the file at `path` as it is now, the one a save renamed over it included, held
/// open in `held`; or while it can't be opened, its folder, for that to change. What was
/// watched before is let go. Says why the file couldn't be opened, if it couldn't.
fn rewatch(
    watcher: &mut Watcher,
    held: &mut Option<File>,
    path: &Path,
    folder: &Path,
    open: &mut impl FnMut(&Path) -> io::Result<File>,
    may: Blocking<'_>,
) -> io::Result<Option<io::ErrorKind>> {
    let changes = FilterFlag::NOTE_WRITE
        | FilterFlag::NOTE_EXTEND
        | FilterFlag::NOTE_ATTRIB
        | FilterFlag::NOTE_DELETE
        | FilterFlag::NOTE_RENAME;
    for _ in 0..TRIES {
        if let Some(file) = held.take() {
            let _ = watcher.remove_fd(file.as_raw_fd(), EventFilter::EVFILT_VNODE);
        }
        let _ = watcher.remove_filename(folder, EventFilter::EVFILT_VNODE);
        let file = match open(path) {
            Ok(file) => file,
            Err(error) => {
                watcher.add_filename(folder, EventFilter::EVFILT_VNODE, FilterFlag::NOTE_WRITE)?;
                watcher.watch()?;
                return Ok(Some(error.kind()));
            }
        };
        watcher.add_fd(file.as_raw_fd(), EventFilter::EVFILT_VNODE, changes)?;
        watcher.watch()?;
        let named = names(path, &file, may);
        *held = Some(file);
        if named {
            return Ok(None);
        }
    }
    Err(io::Error::other("another file was renamed over it each time it was watched"))
}

/// Whether `path` names `file` now: a save renames another over it.
fn names(path: &Path, file: &File, _: Blocking<'_>) -> bool {
    match (std::fs::metadata(path), file.metadata()) {
        (Ok(named), Ok(open)) => (named.dev(), named.ino()) == (open.dev(), open.ino()),
        _ => false,
    }
}

/// Waits for a change to what's watched, then for it to be still for [`SETTLE`].
fn settled(watcher: &Watcher) -> io::Result<()> {
    let mut wait = None;
    while let Some(event) = watcher.poll_forever(wait) {
        if let EventData::Error(error) = event.data
            && error.kind() != io::ErrorKind::Interrupted
        {
            return Err(error);
        }
        wait = Some(SETTLE);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
