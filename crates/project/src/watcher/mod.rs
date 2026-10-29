//! One watcher for the whole process, which every open checkout registers its folders with.
//! The platform is slow to start watching (macOS can take a second for each watcher), so
//! however many checkouts are open, one watcher covers them all; checkouts registering at
//! once are covered by one restart.
//!
//! What the platform reports is copied off its thread at once and sorted on a thread of
//! the hub's own: the platform drops reports it can't hand over, and sorting them waits on
//! each checkout's listings, which its worker may be filing.

#[cfg(target_os = "macos")]
mod fsevents;
#[cfg(not(target_os = "macos"))]
mod portable;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    Arc, Condvar, LazyLock, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard,
    RwLockWriteGuard,
};

#[cfg(target_os = "macos")]
use self::fsevents::Platform;
#[cfg(not(target_os = "macos"))]
use self::portable::Platform;
use crate::places::Places;
use crate::shared::{Counters, Shared};

/// The most reports kept waiting to be sorted. Past it, the platform's thread is outrunning
/// the sorting, and they're thrown away for every checkout to be read again whole, as when
/// the platform loses track itself.
const MOST_WAITING: usize = 200_000;

/// Reports as the platform handed them over: paths, each with whether the platform lost
/// track of what changed under it. A path is `""` when the platform didn't say where.
#[derive(Default)]
pub(crate) struct Batch {
    /// The paths one after another, so a batch is two allocations however many it holds.
    text: String,
    /// Where each path ends in `text`, and whether the platform lost track there.
    reports: Vec<(usize, bool)>,
}

impl Batch {
    pub(crate) fn push(&mut self, path: &str, lost: bool) {
        self.text.push_str(path);
        self.reports.push((self.text.len(), lost));
    }

    fn iter(&self) -> impl Iterator<Item = (&str, bool)> {
        let starts = std::iter::once(0).chain(self.reports.iter().map(|(end, _)| *end));
        (starts.zip(&self.reports)).map(|(start, (end, lost))| (&self.text[start..*end], *lost))
    }
}

/// One checkout's folders, and where what's reported about them goes.
struct Target {
    places: Places,
    shared: Arc<Shared>,
}

struct Hub {
    targets: RwLock<Vec<Arc<Target>>>,
    /// The platform's watcher, once something is watched.
    platform: Mutex<Option<Platform>>,
    waiting: Mutex<Waiting>,
    arrived: Condvar,
}

/// Reports handed over and not sorted yet.
#[derive(Default)]
struct Waiting {
    batches: VecDeque<Batch>,
    reports: usize,
    /// More came than are kept: everything is to be read again.
    overflowed: bool,
}

static HUB: LazyLock<Hub> = LazyLock::new(|| {
    let started = std::thread::Builder::new()
        .name("project watcher".to_owned())
        .spawn(|| HUB.sort());
    if let Err(error) = started {
        log::error!("couldn't start sorting what the platform reports: {error}");
    }
    Hub {
        targets: RwLock::default(),
        platform: Mutex::default(),
        waiting: Mutex::default(),
        arrived: Condvar::new(),
    }
});

/// A checkout's folders, watched until this is dropped.
pub(crate) struct Watching {
    target: Arc<Target>,
}

/// Watches `places` for `shared`: whatever the platform reports about them goes, sorted, to
/// the writes waiting for its worker. Returns once the platform is watching them.
pub(crate) fn watch(shared: &Arc<Shared>, places: Places) -> Result<Watching, String> {
    let target = Arc::new(Target {
        places,
        shared: shared.clone(),
    });
    write(&HUB.targets).push(target.clone());
    // Dropped if the platform can't watch, which takes the target back out.
    let watching = Watching { target };
    HUB.cover()?;
    Ok(watching)
}

impl Drop for Watching {
    fn drop(&mut self) {
        let mut targets = write(&HUB.targets);
        targets.retain(|target| !Arc::ptr_eq(target, &self.target));
        let none = targets.is_empty();
        drop(targets);
        // Folders no longer wanted are left watched until the platform starts again for
        // another checkout: reports about them reach nobody meanwhile.
        if none {
            let mut platform = lock(&HUB.platform);
            if read(&HUB.targets).is_empty() {
                *platform = None;
            }
        }
    }
}

/// Takes `batch` off the platform's thread, to be sorted on the hub's.
pub(crate) fn report(batch: Batch) {
    let mut waiting = lock(&HUB.waiting);
    if waiting.overflowed {
        return;
    }
    waiting.reports += batch.reports.len();
    if waiting.reports > MOST_WAITING {
        *waiting = Waiting {
            overflowed: true,
            ..Waiting::default()
        };
    } else {
        waiting.batches.push_back(batch);
    }
    HUB.arrived.notify_one();
}

impl Hub {
    /// Has the platform watch every folder registered, started again with them all unless
    /// it watches them already. Whoever registered meanwhile is covered too, so checkouts
    /// opened together wait on one start, not one each.
    fn cover(&self) -> Result<(), String> {
        let mut platform = lock(&self.platform);
        let folders = outermost(read(&self.targets).iter().flat_map(|t| t.places.folders()));
        let watched = platform.as_ref().map_or(&[][..], Platform::folders);
        let covered =
            (folders.iter()).all(|folder| watched.iter().any(|outer| folder.starts_with(outer)));
        if covered {
            return Ok(());
        }
        if let Some(platform) = platform.as_mut() {
            return platform.cover(folders);
        }
        *platform = Some(Platform::start(folders)?);
        Ok(())
    }

    /// Sorts what the platform reports, for as long as the process runs.
    fn sort(&self) {
        loop {
            let waiting = {
                let mut waiting = lock(&self.waiting);
                while waiting.batches.is_empty() && !waiting.overflowed {
                    waiting = (self.arrived.wait(waiting)).unwrap_or_else(PoisonError::into_inner);
                }
                std::mem::take(&mut *waiting)
            };
            let mut batches = waiting.batches;
            if waiting.overflowed {
                let mut lost = Batch::default();
                lost.push("", true);
                batches = VecDeque::from([lost]);
            }
            for target in read(&self.targets).iter() {
                target.take(&batches);
            }
        }
    }
}

impl Target {
    /// Sorts what's about this checkout in `batches`, and puts what's worth taking up with
    /// what's waiting for its worker. Its listings are only looked at for a report about it.
    fn take(&self, batches: &VecDeque<Batch>) {
        let (mut heard, mut dropped, mut lost_track, mut seen) = (0, 0, 0, Vec::new());
        let mut tree = None;
        for (path, lost) in batches.iter().flat_map(Batch::iter) {
            if !self.places.hears(path, lost) {
                continue;
            }
            heard += 1;
            let tree = tree.get_or_insert_with(|| self.shared.tree());
            let sorted = if lost {
                self.places.rescan(tree, path)
            } else {
                self.places.seen(tree, path)
            };
            match sorted {
                Some(sorted) => {
                    lost_track += u64::from(lost);
                    seen.push(sorted);
                }
                None => dropped += 1,
            }
        }
        drop(tree);
        if heard == 0 {
            return;
        }
        let counters = &self.shared.counters;
        Counters::add(&counters.events, heard);
        Counters::add(&counters.dropped, dropped);
        // Counted before it's taken up, so whoever is told of it can see why.
        Counters::add(&counters.lost, lost_track);
        self.shared.heard(seen);
    }
}

/// Each of `folders` once, and none inside another: the platform reports a path once for
/// each watched folder it's in.
fn outermost(folders: impl Iterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = folders.collect();
    folders.sort_by_key(|folder| folder.as_os_str().len());
    let mut outermost: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if !outermost.iter().any(|outer| folder.starts_with(outer)) {
            outermost.push(folder);
        }
    }
    outermost
}

/// Takes a lock whoever held it last: what's behind these is whole between statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::entry::{Entry, Kind};
    use crate::ignores::Ignores;
    use crate::tree::Dir;

    #[test]
    fn a_lost_track_is_counted_lost_only_where_a_consumer_reads() {
        let shared = Arc::new(Shared::new("/work/app".into(), 10));
        let node_modules = Entry {
            name: "node_modules".into(),
            kind: Kind::Dir,
            ignored: true,
        };
        let root = Dir {
            entries: Arc::from([node_modules]),
            ignored: false,
            rules: Ignores::excludes(Path::new("/work/app"), &[]),
        };
        shared.tree_mut().put("".into(), root);
        let target = Target {
            places: Places::new(Path::new("/work/app"), None),
            shared: shared.clone(),
        };
        let mut batch = Batch::default();
        batch.push("/work/app/node_modules/react", true);
        batch.push("/work/app/src/a.rs", false);
        batch.push("/work/app", true);
        target.take(&VecDeque::from([batch]));
        let stats = shared.stats();
        assert_eq!((stats.events, stats.dropped, stats.lost), (3, 1, 1));
    }

    #[test]
    fn folders_inside_others_are_watched_through_them() {
        let folders = ["/a/b/.git", "/a/b", "/c", "/a/bc", "/c"].map(PathBuf::from);
        assert_eq!(
            outermost(folders.into_iter()),
            ["/c", "/a/b", "/a/bc"].map(PathBuf::from)
        );
    }

    #[test]
    fn a_batch_gives_back_its_paths_in_order() {
        let mut batch = Batch::default();
        batch.push("/a/b", false);
        batch.push("", true);
        batch.push("/c", false);
        let paths: Vec<(&str, bool)> = batch.iter().collect();
        assert_eq!(paths, [("/a/b", false), ("", true), ("/c", false)]);
    }
}
