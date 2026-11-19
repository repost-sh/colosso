use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::{App, Global};

use super::write_atomic;
use crate::blocking::{self, Blocking};

/// How long the app quitting, or a window opening again, waits on a write that's running
/// before it goes on without it.
pub(crate) const WAITS: Duration = Duration::from_secs(1);

/// The one writer of each file the app keeps: a write lands after those handed to it before,
/// and one still waiting when the next comes is dropped for it, as the next holds what it did.
/// Writes run on the background executor; what's still waiting when the app quits is written
/// there and then, each file's once the write of it running ends ([`Writers::settle`]).
#[derive(Clone, Default)]
pub struct Writers(Arc<Shared>);

impl Global for Writers {}

struct Shared {
    files: Mutex<HashMap<PathBuf, File>>,
    /// Told each time a write ends.
    ended: Condvar,
    /// How a file is written: [`write_atomic`], or a test's stand-in for the disk.
    write: Write,
}

type Write = Box<dyn Fn(&Path, &[u8], Blocking<'_>) + Send + Sync>;

impl Default for Shared {
    fn default() -> Self {
        Self::writing_with(Box::new(write_atomic))
    }
}

impl Shared {
    fn writing_with(write: Write) -> Self {
        Shared {
            files: Mutex::default(),
            ended: Condvar::new(),
            write,
        }
    }
}

/// One file's writes.
#[derive(Default)]
struct File {
    /// What's to be written next, the latest handed.
    next: Option<Due>,
    /// Whether it's being written now.
    writing: bool,
    /// Whether a task on the background executor is to write what comes next.
    draining: bool,
}

/// The JSON to write, and who's told once it's landed.
struct Due {
    json: Vec<u8>,
    landed: Vec<oneshot::Sender<()>>,
}

impl Writers {
    pub fn of(cx: &mut App) -> Self {
        cx.default_global::<Self>().clone()
    }

    /// Hands `json` to `path`'s writer, which writes it once the write of the file running
    /// ends, unless a later one comes first. What it returns is ready once the file holds it,
    /// or the later one.
    pub fn write(&self, path: PathBuf, json: Vec<u8>, cx: &App) -> oneshot::Receiver<()> {
        let (told, drain) = self.hand(path.clone(), json);
        if drain {
            let writers = self.clone();
            blocking::spawn(cx.background_executor(), move |may| writers.drain(&path, may))
                .detach();
        }
        told
    }

    /// Hands `json` to `path`'s writer: what's ready once the file holds it, and whether a
    /// drain is to start, as none is under way for the file.
    fn hand(&self, path: PathBuf, json: Vec<u8>) -> (oneshot::Receiver<()>, bool) {
        let (landed, told) = oneshot::channel();
        let mut files = self.files();
        let file = files.entry(path).or_default();
        match &mut file.next {
            Some(due) => {
                due.json = json;
                due.landed.push(landed);
            }
            None => {
                file.next = Some(Due {
                    json,
                    landed: vec![landed],
                });
            }
        }
        let drain = !file.draining;
        file.draining = true;
        (told, drain)
    }

    /// Writes what's waiting for `path` until nothing is.
    fn drain(&self, path: &Path, may: Blocking<'_>) {
        loop {
            let mut files = self.files();
            while files.get(path).is_some_and(|file| file.writing) {
                files = (self.0.ended.wait(files)).unwrap_or_else(PoisonError::into_inner);
            }
            let Some(file) = files.get_mut(path) else {
                return;
            };
            let Some(due) = file.next.take() else {
                file.draining = false;
                return;
            };
            file.writing = true;
            drop(files);
            self.land(path, due, may);
        }
    }

    /// Writes everything waiting, once the writes running end, waiting on them for `WAITS`
    /// at most: as the app quits, and before a window opening again reads its files. A file
    /// still being written then is gone on without, and its write waiting with it; every other
    /// file's waiting write lands.
    pub fn settle(&self, may: Blocking<'_>) {
        let until = Instant::now() + WAITS;
        let mut files = self.files();
        loop {
            if let Some((path, due)) = next_to_land(&mut files) {
                drop(files);
                self.land(&path, due, may);
                files = self.files();
                continue;
            }
            if !files.values().any(|file| file.writing) {
                return;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                for path in (files.iter()).filter_map(|(path, file)| file.writing.then_some(path)) {
                    log::warn!("{} was still being written: going on without it", path.display());
                }
                return;
            }
            let waited = self.0.ended.wait_timeout(files, left);
            files = waited.unwrap_or_else(PoisonError::into_inner).0;
        }
    }

    /// Writes `due` to `path`, which is noted as being written, and tells who waits on it.
    fn land(&self, path: &Path, due: Due, may: Blocking<'_>) {
        (self.0.write)(path, &due.json, may);
        if let Some(file) = self.files().get_mut(path) {
            file.writing = false;
        }
        self.0.ended.notify_all();
        for landed in due.landed {
            let _ = landed.send(());
        }
    }

    fn files(&self) -> MutexGuard<'_, HashMap<PathBuf, File>> {
        self.0.files.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The write waiting for a file not being written now, if there's one, the file noted as being
/// written.
fn next_to_land(files: &mut HashMap<PathBuf, File>) -> Option<(PathBuf, Due)> {
    let (path, file) = (files.iter_mut()).find(|(_, file)| !file.writing && file.next.is_some())?;
    let due = file.next.take()?;
    file.writing = true;
    Some((path.clone(), due))
}

#[cfg(test)]
pub(super) mod stuck;
#[cfg(test)]
mod tests;
