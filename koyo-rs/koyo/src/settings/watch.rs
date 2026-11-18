//! The settings file, watched for an edit made outside the app, in an editor beside it say, on
//! a thread of its own: a kqueue waits on the file, which an edit in place changes, as does a
//! save that renames a new file over it (the file watched goes, and the new one is watched
//! from then on). Its folder is watched only while there's no file, for one to come, so the
//! app's other files there, written as the app runs, don't wake it. Once what woke it settles,
//! the thread asks when the file last changed, and only when that moved reads it and hands it
//! to the main thread. Nothing wakes while the file is left alone, and an edit lands whether
//! Koyo is in front or not. `FSEvents` isn't used: it would wake for every write in the
//! folder's tree, the browser's profiles among them.

use std::io;
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

/// Watches the file at `path`, last changed at `seen` as the app read it, taking up each edit
/// made outside the app.
pub(super) fn start(path: PathBuf, seen: Option<SystemTime>, cx: &App) {
    let (tell, mut told) = futures::channel::mpsc::unbounded();
    let started = blocking::thread("settings watch", move |may| {
        let watched = watch(&path, seen, may, |looked| {
            tell.unbounded_send(looked).is_ok()
        });
        if let Err(error) = watched {
            log::warn!("stopped watching {}: {error}", path.display());
        }
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
/// `seen`, until `tell` answers that nobody listens.
pub(super) fn watch(
    path: &Path,
    mut seen: Option<SystemTime>,
    may: Blocking<'_>,
    mut tell: impl FnMut(Looked) -> bool,
) -> io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(folder)?;
    let mut watcher = Watcher::new()?;
    loop {
        let on_file = rewatch(&mut watcher, path, folder)?;
        let modified = file::modified(path, may);
        if modified != seen {
            seen = modified;
            if !tell(file::look(path, modified, may)) {
                return Ok(());
            }
        }
        if on_file || modified.is_none() {
            settled(&watcher)?;
        }
    }
}

/// Watches the file at `path` as it is now, the one a save renamed over it included, or while
/// there's none, its folder, for the file to come; what was watched before is let go. Says
/// whether it watches the file.
fn rewatch(watcher: &mut Watcher, path: &Path, folder: &Path) -> io::Result<bool> {
    let _ = watcher.remove_filename(path, EventFilter::EVFILT_VNODE);
    let _ = watcher.remove_filename(folder, EventFilter::EVFILT_VNODE);
    let changes = FilterFlag::NOTE_WRITE
        | FilterFlag::NOTE_EXTEND
        | FilterFlag::NOTE_ATTRIB
        | FilterFlag::NOTE_DELETE
        | FilterFlag::NOTE_RENAME;
    let on_file = (watcher.add_filename(path, EventFilter::EVFILT_VNODE, changes)).is_ok();
    if !on_file {
        watcher.add_filename(folder, EventFilter::EVFILT_VNODE, FilterFlag::NOTE_WRITE)?;
    }
    watcher.watch()?;
    Ok(on_file)
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
