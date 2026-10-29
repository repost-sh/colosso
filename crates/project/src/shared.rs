//! What the watcher, the worker and the project's handles share: the listings, the writes
//! waiting to be taken up, the subscribers, and counts of the work done.

use std::collections::HashSet;
use std::ops::Sub;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{
    Arc, Condvar, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak,
};
use std::time::{Duration, Instant};

use crate::feed::{Feed, Git, Inbox, Scanned, Update};
use crate::files::{Files, Folder};
use crate::lock;
use crate::paths::Written;
use crate::tree::Tree;

/// What the platform reported that's worth taking up.
#[derive(Debug)]
pub(crate) enum Seen {
    /// A path under the root, relative to it.
    Path(Arc<str>),
    /// Something consumers read in the git folders.
    Git(Git),
    /// A folder whose every part may have changed: the platform lost track of what.
    Rescan(Arc<str>),
    /// Anything may have changed: the platform lost track of the whole checkout, or the
    /// repository's excludes, which say what's ignored everywhere, were written.
    Everything,
}

/// What's waiting for the worker.
#[derive(Debug, Default)]
pub(crate) struct Pending {
    pub written: Written,
    pub git: Git,
    /// Ignored folders a consumer asked to read.
    pub loads: Vec<Arc<str>>,
    /// Folders to read again, and everything listed under them.
    pub rescans: HashSet<Arc<str>>,
    /// When the first write waiting was reported.
    since: Option<Instant>,
    /// When the platform last reported anything about the checkout, dropped or not.
    heard: Option<Instant>,
    stopped: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.written.is_empty()
            && self.git.is_empty()
            && self.loads.is_empty()
            && self.rescans.is_empty()
    }
}

/// Counts of what a project has done, for measuring it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Reports from the platform's watcher about the checkout.
    pub events: u64,
    /// Of those, the ones dropped where they arrived: inside ignored folders nobody reads,
    /// or among git's own files that nothing shows.
    pub dropped: u64,
    /// Of those, the ones saying the platform lost track of what changed somewhere a
    /// consumer reads, which is read again whole: it drops reports when it falls behind.
    pub lost: u64,
    /// Batches of writes taken up.
    pub batches: u64,
    /// Folders read from the disk, the first scan's included.
    pub folders_read: u64,
    /// Time spent taking up batches.
    pub busy: Duration,
}

/// What a project did between two readings of its counts.
impl Sub for Stats {
    type Output = Stats;

    fn sub(self, before: Stats) -> Stats {
        Stats {
            events: self.events.saturating_sub(before.events),
            dropped: self.dropped.saturating_sub(before.dropped),
            lost: self.lost.saturating_sub(before.lost),
            batches: self.batches.saturating_sub(before.batches),
            folders_read: self.folders_read.saturating_sub(before.folders_read),
            busy: self.busy.saturating_sub(before.busy),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub events: AtomicU64,
    pub dropped: AtomicU64,
    pub lost: AtomicU64,
    pub batches: AtomicU64,
    pub folders_read: AtomicU64,
    pub busy_ns: AtomicU64,
}

impl Counters {
    pub(crate) fn add(counter: &AtomicU64, by: u64) {
        counter.fetch_add(by, Ordering::Relaxed);
    }

    fn stats(&self) -> Stats {
        let get = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        Stats {
            events: get(&self.events),
            dropped: get(&self.dropped),
            lost: get(&self.lost),
            batches: get(&self.batches),
            folders_read: get(&self.folders_read),
            busy: Duration::from_nanos(get(&self.busy_ns)),
        }
    }
}

pub(crate) struct Shared {
    pub root: PathBuf,
    /// How many written paths, then folders, are kept by name.
    pub most: usize,
    tree: RwLock<Tree>,
    pending: Mutex<Pending>,
    wake: Condvar,
    subscribers: Mutex<Vec<Weak<Inbox>>>,
    pub counters: Counters,
    /// Set when the project is dropped, for the worker and readers to stop.
    pub stopped: AtomicBool,
    unwatched: Mutex<Option<String>>,
    scanned: Mutex<Option<Scanned>>,
    files: Mutex<Files>,
}

impl Shared {
    pub(crate) fn new(root: PathBuf, most: usize) -> Self {
        Shared {
            root,
            most,
            tree: RwLock::default(),
            pending: Mutex::default(),
            wake: Condvar::new(),
            subscribers: Mutex::default(),
            counters: Counters::default(),
            stopped: false.into(),
            unwatched: Mutex::default(),
            scanned: Mutex::default(),
            files: Mutex::default(),
        }
    }

    pub(crate) fn tree(&self) -> RwLockReadGuard<'_, Tree> {
        self.tree.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn tree_mut(&self) -> RwLockWriteGuard<'_, Tree> {
        self.tree.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Notes that the platform reported about the checkout, and puts what's worth taking up
    /// with what's waiting, waking the worker if nothing was: one already waiting has been
    /// woken.
    pub(crate) fn heard(&self, seen: Vec<Seen>) {
        let mut pending = lock(&self.pending);
        pending.heard = Some(Instant::now());
        if seen.is_empty() {
            return;
        }
        let idle = pending.is_empty();
        pending.since.get_or_insert_with(Instant::now);
        for seen in seen {
            match seen {
                Seen::Path(path) => pending.written.insert(path, self.most),
                Seen::Git(git) => pending.git.merge(git),
                Seen::Rescan(dir) => {
                    pending.rescans.insert(dir);
                    if pending.rescans.len() > self.most {
                        pending.rescans.clear();
                        pending.written = Written::Everything;
                    }
                }
                Seen::Everything => {
                    pending.rescans.clear();
                    pending.written = Written::Everything;
                }
            }
        }
        if idle {
            self.wake.notify_one();
        }
    }

    pub(crate) fn load(&self, dir: Arc<str>) {
        lock(&self.pending).loads.push(dir);
        self.wake.notify_one();
    }

    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        lock(&self.pending).stopped = true;
        self.wake.notify_one();
    }

    /// What's waiting, once there's something: a folder asked for at once; writes once
    /// `window` has passed since the first of them, so a burst is taken up together; and a
    /// folder, or everything, to read again whole once nothing has been reported about the
    /// checkout for `window`, or `longest` has passed. `None` once the project is dropped.
    pub(crate) fn next_batch(&self, window: Duration, longest: Duration) -> Option<Pending> {
        let mut pending = lock(&self.pending);
        loop {
            if pending.stopped {
                return None;
            }
            if !pending.loads.is_empty() {
                break;
            }
            let Some(since) = pending.since.filter(|_| !pending.is_empty()) else {
                pending = (self.wake.wait(pending)).unwrap_or_else(PoisonError::into_inner);
                continue;
            };
            // Reading whole folders again costs as much as the first scan, and the platform
            // loses track in the middle of a burst, again and again: they're read once it's
            // over, rather than each time.
            let whole = pending.written == Written::Everything || !pending.rescans.is_empty();
            let left = if whole {
                let quiet = (pending.heard).map_or(Duration::ZERO, |heard| {
                    window.saturating_sub(heard.elapsed())
                });
                quiet.min(longest.saturating_sub(since.elapsed()))
            } else {
                window.saturating_sub(since.elapsed())
            };
            if left.is_zero() {
                break;
            }
            pending = (self.wake.wait_timeout(pending, left))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Some(std::mem::take(&mut *pending))
    }

    pub(crate) fn subscribe(&self, feed: Feed) -> Arc<Inbox> {
        let mut subscribers = lock(&self.subscribers);
        // Under the subscribers' lock, so no update published meanwhile is missed.
        let mut first = Update {
            unwatched: self.unwatched(),
            ..Update::default()
        };
        if feed.listed() {
            let tree = self.tree();
            first.listings = (tree.dirs.iter())
                .map(|(dir, listed)| (dir.clone(), Some(listed.entries.clone())))
                .collect();
            first.scanned = self.scanned();
        }
        let inbox = Inbox::new(feed, self.most, &first);
        subscribers.retain(|subscriber| subscriber.strong_count() > 0);
        subscribers.push(Arc::downgrade(&inbox));
        inbox
    }

    /// Hands `update` to every subscriber that wants some of it.
    pub(crate) fn publish(&self, update: &Update) {
        if update.is_empty() {
            return;
        }
        let subscribers = lock(&self.subscribers);
        for inbox in subscribers.iter().filter_map(Weak::upgrade) {
            inbox.deliver(update);
        }
    }

    pub(crate) fn set_unwatched(&self, why: String) {
        *lock(&self.unwatched) = Some(why.clone());
        self.publish(&Update {
            unwatched: Some(why),
            ..Update::default()
        });
    }

    pub(crate) fn unwatched(&self) -> Option<String> {
        lock(&self.unwatched).clone()
    }

    pub(crate) fn set_scanned(&self, scanned: Scanned) {
        *lock(&self.scanned) = Some(scanned);
        self.publish(&Update {
            scanned: Some(scanned),
            ..Update::default()
        });
    }

    pub(crate) fn scanned(&self) -> Option<Scanned> {
        *lock(&self.scanned)
    }

    pub(crate) fn stats(&self) -> Stats {
        self.counters.stats()
    }

    /// Every file listed that git doesn't ignore.
    pub(crate) fn files(&self) -> Arc<[Arc<str>]> {
        self.with_files(Files::list)
    }

    /// The same, by folder.
    pub(crate) fn folders(&self) -> Arc<[Folder]> {
        self.with_files(Files::folders)
    }

    /// `read` of the files as the tree lists them now. The listings are only looked at under
    /// the tree's lock; the files are made after it's let go, so the worker isn't held up.
    fn with_files<R>(&self, read: impl FnOnce(&mut Files) -> R) -> R {
        let mut files = lock(&self.files);
        let listed = {
            let tree = self.tree();
            (!files.is_at(tree.generation)).then(|| (tree.generation, tree.listings()))
        };
        if let Some((generation, listings)) = listed {
            files.update(generation, listings);
        }
        read(&mut files)
    }
}
