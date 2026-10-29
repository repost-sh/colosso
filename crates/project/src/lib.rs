//! One checkout's files, kept in step with the disk off the main thread: each folder's
//! listing, which entries git ignores, and what was written, for every panel that shows the
//! checkout. The checkout is scanned once and watched once however many consumers subscribe,
//! and nothing here knows about GPUI.
//!
//! Folders git ignores (`node_modules`, `target`) are listed as entries but not read until a
//! consumer asks, and writes inside them are dropped where the platform reports them, so an
//! `npm install` or a `cargo build` wakes nobody.

mod entry;
mod feed;
mod files;
mod ignores;
mod paths;
mod places;
mod read;
mod scan;
mod shared;
mod tree;
mod watcher;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use entry::{Entry, Kind, natural, order};
pub use feed::{Counts, Feed, Git, Scanned, Subscription, Update};
pub use ignores::Excludes;
pub use paths::Written;
pub use shared::Stats;

use crate::shared::Shared;

/// How many written paths are kept by name before the folders they're in stand for them,
/// and how many folders before everything does.
const MOST_WRITTEN: usize = 10_000;

/// A checkout's git folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitDirs {
    /// The checkout's own, with its index and `HEAD`.
    pub git_dir: PathBuf,
    /// The repository's, shared by its worktrees: the branches and `info/exclude`. A main
    /// checkout's two are one.
    pub common_dir: PathBuf,
}

impl GitDirs {
    /// A main checkout's, at `root`: both are its `.git`.
    pub fn main(root: &Path) -> GitDirs {
        let git = root.join(".git");
        GitDirs {
            git_dir: git.clone(),
            common_dir: git,
        }
    }
}

/// What to open, and how.
#[derive(Clone, Debug)]
pub struct Options {
    pub root: PathBuf,
    /// `None` outside a repository: then only `.gitignore` files say what's ignored.
    pub git: Option<GitDirs>,
    /// The user's own excludes, under every other rule.
    pub excludes: Excludes,
    /// How many written paths are kept by name before their folders stand for them, and how
    /// many folders before everything does.
    pub most_written: usize,
}

impl Options {
    /// The checkout at `root`, with git's own excludes and the usual limits.
    pub fn new(root: PathBuf, git: Option<GitDirs>) -> Self {
        Options {
            root,
            git,
            excludes: Excludes::Git,
            most_written: MOST_WRITTEN,
        }
    }
}

/// One checkout, scanned and watched on a thread of its own until the last handle to it is
/// dropped.
pub struct Project {
    shared: Arc<Shared>,
}

impl Project {
    /// Starts scanning and watching the checkout `options` names, and returns at once.
    pub fn open(options: Options) -> Arc<Project> {
        let shared = Arc::new(Shared::new(
            options.root.clone(),
            options.most_written.max(1),
        ));
        let worker = shared.clone();
        let started = std::thread::Builder::new()
            .name("project".to_owned())
            .spawn(move || worker::run(worker, &options));
        if let Err(error) = started {
            shared.set_unwatched(format!("couldn't start reading the checkout: {error}"));
        }
        Arc::new(Project { shared })
    }

    pub fn root(&self) -> &Path {
        &self.shared.root
    }

    /// Updates from here on, first with what's known already: every listing, for `Everything`.
    pub fn subscribe(self: &Arc<Self>, feed: Feed) -> Subscription {
        Subscription {
            inbox: self.shared.subscribe(feed),
            project: self.clone(),
        }
    }

    /// Reads the ignored folder `dir`, one level of it: its own subfolders are listed, and
    /// read when they're asked for in turn. It's kept listed from then on.
    pub fn load(&self, dir: &str) {
        self.shared.load(dir.into());
    }

    /// The listing of `dir`, if it's been read.
    pub fn listing(&self, dir: &str) -> Option<Arc<[Entry]>> {
        let tree = self.shared.tree();
        tree.dirs.get(dir).map(|listed| listed.entries.clone())
    }

    /// Every file listed that git doesn't ignore, by path, in no particular order: what a
    /// fuzzy finder searches. Made again only when a listing has changed since it was last
    /// asked for, and shared until then.
    pub fn files(&self) -> Arc<[Arc<str>]> {
        self.shared.files()
    }

    /// The first scan's counts, once it's done.
    pub fn scanned(&self) -> Option<Scanned> {
        self.shared.scanned()
    }

    /// Why writes aren't followed, if the platform couldn't watch the checkout.
    pub fn unwatched(&self) -> Option<String> {
        self.shared.unwatched()
    }

    pub fn stats(&self) -> Stats {
        self.shared.stats()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        // The worker stops between folders, and the watcher with it; nothing waits for that.
        self.shared.stop();
    }
}
