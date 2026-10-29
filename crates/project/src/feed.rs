//! What subscribers are told. Each has an inbox that updates are merged into rather than
//! queued in: a listing replaces the one before it, written paths are kept to a bound, so a
//! subscriber that's slow to look takes one update, never a backlog.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Stream;
use futures::task::AtomicWaker;

use crate::entry::Entry;
use crate::paths::Written;
use crate::{Project, lock};

/// What a subscriber wants to hear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feed {
    /// What's written: for a consumer that reads files rather than lists them, as the
    /// changes panel does.
    Written,
    /// The listings, as they're read, and the first scan's end as well: for one that shows
    /// the folders.
    Everything,
    /// The listings and the first scan's end alone: for one that only needs to know which
    /// files there are, as Go to File does.
    Listings,
}

impl Feed {
    /// Whether it's told the listings.
    pub(crate) fn listed(self) -> bool {
        self != Feed::Written
    }

    /// Whether it's told what's written.
    fn written(self) -> bool {
        self != Feed::Listings
    }
}

/// What happened since a subscriber last took an update.
#[derive(Clone, Debug, Default)]
pub struct Update {
    /// Folders listed anew, each in place of whatever was listed for it before; `None` for a
    /// folder gone, or no longer listed now that git ignores it.
    pub listings: HashMap<Arc<str>, Option<Arc<[Entry]>>>,
    /// Files and folders written, created or removed, outside the folders git ignores.
    pub written: Written,
    /// What moved in the git folders.
    pub git: Git,
    /// The first scan is done: everything git doesn't ignore is listed.
    pub scanned: Option<Scanned>,
    /// Why writes aren't followed, if the platform couldn't watch the checkout.
    pub unwatched: Option<String>,
}

/// What moved in a checkout's git folders, of what consumers read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Git {
    /// The index: what's staged, and what unstaged changes are compared with.
    pub index: bool,
    /// `HEAD`, or a branch it may name (under `refs/heads/`, or in `packed-refs`): the
    /// commit the files are compared with may have moved.
    pub head: bool,
}

impl Git {
    /// Everything consumers read, for when the platform lost track of a git folder.
    pub(crate) const ALL: Git = Git {
        index: true,
        head: true,
    };

    pub fn is_empty(self) -> bool {
        !self.index && !self.head
    }

    pub(crate) fn merge(&mut self, other: Git) {
        self.index |= other.index;
        self.head |= other.head;
    }
}

/// How much is listed: folders, and files git doesn't ignore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub folders: usize,
    pub files: usize,
}

/// The first scan, done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scanned {
    pub counts: Counts,
    pub elapsed: Duration,
}

impl Update {
    pub fn is_empty(&self) -> bool {
        self.listings.is_empty()
            && self.written.is_empty()
            && self.git.is_empty()
            && self.scanned.is_none()
            && self.unwatched.is_none()
    }

    /// What `feed` wants of it.
    fn wanted(&self, feed: Feed) -> Update {
        let (listed, written) = (feed.listed(), feed.written());
        Update {
            listings: if listed {
                self.listings.clone()
            } else {
                HashMap::new()
            },
            written: if written {
                self.written.clone()
            } else {
                Written::default()
            },
            git: if written { self.git } else { Git::default() },
            scanned: self.scanned.filter(|_| listed),
            unwatched: self.unwatched.clone(),
        }
    }

    /// Adds to `self` what `feed` wants of `other`; whether that was anything.
    fn merge(&mut self, other: &Update, feed: Feed, most: usize) -> bool {
        if self.is_empty() {
            *self = other.wanted(feed);
            return !self.is_empty();
        }
        let mut added = false;
        if feed.written() {
            added = !other.written.is_empty() || !other.git.is_empty();
            self.written.merge(&other.written, most);
            self.git.merge(other.git);
        }
        if let Some(why) = &other.unwatched {
            self.unwatched = Some(why.clone());
            added = true;
        }
        if feed.listed() {
            for (dir, listing) in &other.listings {
                self.listings.insert(dir.clone(), listing.clone());
                added = true;
            }
            if other.scanned.is_some() {
                self.scanned = other.scanned;
                added = true;
            }
        }
        added
    }
}

/// Where one subscriber's updates wait for it.
pub(crate) struct Inbox {
    feed: Feed,
    most: usize,
    update: Mutex<Update>,
    /// For a thread waiting on it.
    ready: Condvar,
    /// For a task waiting on it.
    waker: AtomicWaker,
}

impl Inbox {
    pub(crate) fn new(feed: Feed, most: usize, first: &Update) -> Arc<Inbox> {
        let inbox = Inbox {
            feed,
            most,
            update: Mutex::new(Update::default()),
            ready: Condvar::new(),
            waker: AtomicWaker::new(),
        };
        inbox.deliver(first);
        Arc::new(inbox)
    }

    /// Adds what this subscriber wants of `update`.
    pub(crate) fn deliver(&self, update: &Update) {
        if self.update().merge(update, self.feed, self.most) {
            self.ready.notify_all();
            self.waker.wake();
        }
    }

    fn update(&self) -> MutexGuard<'_, Update> {
        lock(&self.update)
    }

    fn take(update: &mut Update) -> Option<Update> {
        (!update.is_empty()).then(|| std::mem::take(update))
    }
}

/// One consumer's updates from a project, which it keeps open while held. Read as a
/// `Stream` from a task, or with `next_within` or `until` from a thread.
pub struct Subscription {
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) project: Arc<Project>,
}

impl Subscription {
    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    /// Everything that's happened since the last update taken, if anything has.
    pub fn try_next(&self) -> Option<Update> {
        Inbox::take(&mut self.inbox.update())
    }

    /// The next update, waiting up to `wait` for one.
    pub fn next_within(&self, wait: Duration) -> Option<Update> {
        let deadline = Instant::now() + wait;
        let mut update = self.inbox.update();
        loop {
            if let Some(taken) = Inbox::take(&mut update) {
                return Some(taken);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            update = (self.inbox.ready.wait_timeout(update, left))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Takes updates until `wanted` accepts one; whether one came before `deadline`.
    pub fn until(&self, deadline: Instant, mut wanted: impl FnMut(&Update) -> bool) -> bool {
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.next_within(left) {
                Some(update) if wanted(&update) => return true,
                Some(_) => {}
                None => return false,
            }
        }
        false
    }
}

impl Stream for Subscription {
    type Item = Update;

    /// Never ends: the subscription keeps its project open.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Update>> {
        self.inbox.waker.register(cx.waker());
        match self.try_next() {
            Some(update) => Poll::Ready(Some(update)),
            None => Poll::Pending,
        }
    }
}
