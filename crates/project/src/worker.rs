//! A project's worker: one thread for the checkout that scans it first, then takes up what
//! the watcher reports a batch at a time, reading again only the folders written in.

use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Options;
use crate::entry::{Kind, find};
use crate::feed::{Counts, Scanned, Update};
use crate::ignores::Ignores;
use crate::paths::{Written, join, name, parent};
use crate::places::Places;
use crate::read::{Before, Job, Read};
use crate::scan::scan;
use crate::shared::{Counters, Pending, Shared};
use crate::tree::{Dir, Tree};
use crate::watcher::watch;

/// How long writes are gathered after the first before they're taken up together: the
/// platform hands each over as it comes, and a save, a checkout or a build writes many files
/// within a few milliseconds.
const WINDOW: Duration = Duration::from_millis(40);

/// How long reading a folder whole, or everything, again waits at most for the checkout to go
/// quiet: a burst that never ends is still read once a second.
const LONGEST: Duration = Duration::from_secs(1);

/// The most threads that read folders at once: past this, the disk is the limit.
const MOST_THREADS: usize = 8;

/// How often the first scan hands over the listings read so far.
const PUBLISH_EVERY: Duration = Duration::from_millis(100);

/// Watches and scans the checkout `options` names until the project is dropped.
pub(crate) fn run(shared: Arc<Shared>, options: &Options) {
    let places = Places::new(&options.root, options.git.as_ref());
    // Watching starts first, so nothing written while the scan goes on is missed.
    let watching = match watch(&shared, places) {
        Ok(watching) => Some(watching),
        Err(error) => {
            shared.set_unwatched(error);
            None
        }
    };
    let mut worker = Worker::new(shared, options);
    worker.first_scan();
    while let Some(batch) = worker.shared.next_batch(WINDOW, LONGEST) {
        worker.take_up(batch);
    }
    drop(watching);
}

struct Worker {
    shared: Arc<Shared>,
    threads: usize,
    /// The files whose rules go under every `.gitignore`.
    excludes: Vec<PathBuf>,
    /// The rules read from them.
    base: Arc<Ignores>,
}

impl Worker {
    fn new(shared: Arc<Shared>, options: &Options) -> Self {
        let repository =
            (options.git.as_ref()).map(|git| git.common_dir.join("info").join("exclude"));
        let excludes: Vec<PathBuf> = repository
            .into_iter()
            .chain(options.excludes.path())
            .collect();
        let cores = std::thread::available_parallelism().map_or(1, NonZero::get);
        Worker {
            base: Ignores::excludes(&shared.root, &excludes),
            shared,
            threads: cores.min(MOST_THREADS),
            excludes,
        }
    }

    /// The root, to be read and, if `deep`, everything listed under it too.
    fn root(&self, tree: &Tree, deep: bool) -> Job {
        Job {
            dir: "".into(),
            ignored: false,
            above: self.base.clone(),
            deep,
            before: before(tree, ""),
        }
    }

    /// Lists every folder git doesn't ignore, handing the listings to subscribers as it goes.
    fn first_scan(&self) {
        let started = Instant::now();
        let mut told = Instant::now();
        let mut update = Update::default();
        let shared = &self.shared;
        let root = self.root(&shared.tree(), true);
        self.read(vec![root], &mut update, |update| {
            if told.elapsed() >= PUBLISH_EVERY {
                shared.publish(&std::mem::take(update));
                told = Instant::now();
            }
        });
        shared.publish(&update);
        let counts = {
            let tree = shared.tree();
            Counts {
                folders: tree.dirs.len(),
                files: tree.files,
            }
        };
        let elapsed = started.elapsed();
        log::info!(
            "listed {}: {} folders and {} files in {elapsed:.1?}",
            shared.root.display(),
            counts.folders,
            counts.files,
        );
        shared.set_scanned(Scanned { counts, elapsed });
    }

    /// Takes up one batch: reads again the folders written in, and those asked for, and tells
    /// subscribers what changed and what was written outside the ignored folders.
    fn take_up(&mut self, batch: Pending) {
        let started = Instant::now();
        Counters::add(&self.shared.counters.batches, 1);
        let Pending {
            mut written,
            git,
            loads,
            rescans,
            ..
        } = batch;
        let mut jobs = self.loads(&loads);
        if written == Written::Everything {
            // The excludes may be what changed.
            self.base = Ignores::excludes(&self.shared.root, &self.excludes);
            jobs.push(self.root(&self.shared.tree(), true));
        } else {
            jobs.extend(self.relists(&written, &rescans));
            for dir in rescans {
                written.insert_folder(dir, self.shared.most);
            }
        }
        let mut update = Update::default();
        self.read(jobs, &mut update, |_| {});
        {
            let tree = self.shared.tree();
            written.retain(|path| !tree.ignores(path));
        }
        update.written = written;
        update.git = git;
        self.shared.publish(&update);
        let busy = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        Counters::add(&self.shared.counters.busy_ns, busy);
    }

    /// The ignored folders asked for that can be read: listed in their parent, and not yet
    /// read themselves. They stay listed from then on.
    fn loads(&self, loads: &[Arc<str>]) -> Vec<Job> {
        let mut tree = self.shared.tree_mut();
        let mut jobs = Vec::new();
        for dir in loads {
            if tree.dirs.contains_key(dir) {
                continue;
            }
            let Some(above) = tree.dirs.get(parent(dir)) else {
                continue;
            };
            let Some(entry) = find(&above.entries, name(dir)).filter(|e| e.kind == Kind::Dir)
            else {
                continue;
            };
            let job = Job {
                dir: dir.clone(),
                ignored: entry.ignored,
                above: above.rules.clone(),
                deep: false,
                before: None,
            };
            tree.loaded.insert(dir.clone());
            jobs.push(job);
        }
        jobs
    }

    /// The folders to read again for what was `written`: each one written in, or the nearest
    /// listed above it when it's new; and those to `rescan` with everything under them.
    fn relists(&self, written: &Written, rescan: &HashSet<Arc<str>>) -> Vec<Job> {
        let tree = self.shared.tree();
        let folders: Vec<&str> = match written {
            Written::Paths(paths) => paths.iter().map(|path| parent(path)).collect(),
            Written::Folders(folders) => folders.iter().map(|folder| &**folder).collect(),
            Written::Everything => Vec::new(),
        };
        let mut dirs: HashMap<Arc<str>, bool> = HashMap::new();
        let shallow = folders.into_iter().map(|folder| (folder, false));
        for (folder, deep) in shallow.chain(rescan.iter().map(|dir| (&**dir, true))) {
            if let Some(listed) = tree.nearest_listed(folder) {
                *dirs.entry(listed).or_default() |= deep;
            }
        }
        (dirs.into_iter())
            .filter_map(|(dir, deep)| {
                Some(Job {
                    ignored: tree.dirs.get(&dir)?.ignored,
                    above: tree.above(&dir, &self.base)?,
                    deep,
                    before: before(&tree, &dir),
                    dir,
                })
            })
            .collect()
    }

    /// Reads `jobs`, and the folders they lead to, filing them and adding what changed to
    /// `update`; `tick` sees the update after each batch filed.
    fn read(&self, jobs: Vec<Job>, update: &mut Update, mut tick: impl FnMut(&mut Update)) {
        let shared = &self.shared;
        scan(&shared.root, self.threads, jobs, &shared.stopped, |reads| {
            Counters::add(&shared.counters.folders_read, reads.len() as u64);
            let mut next = Vec::new();
            {
                let mut tree = shared.tree_mut();
                for read in reads {
                    file(&mut tree, read, update, &mut next);
                }
            }
            tick(update);
            next
        });
    }
}

/// `dir` as last read, if it's listed.
fn before(tree: &Tree, dir: &str) -> Option<Before> {
    tree.dirs.get(dir).map(|listed| Before {
        entries: listed.entries.clone(),
        rules: listed.rules.clone(),
    })
}

/// Files a folder as read, and adds to `next` the folders under it to read: new ones, ones
/// whose rules changed, and all of them for a deep read. Folders gone, or ignored now and
/// not asked for, are taken out.
fn file(tree: &mut Tree, read: Read, update: &mut Update, next: &mut Vec<Job>) {
    let Read {
        job,
        rules,
        entries,
    } = read;
    // Its parent was found gone while it was being read: it went with it.
    let orphaned = !job.dir.is_empty() && !tree.dirs.contains_key(parent(&job.dir));
    let Some(entries) = entries.filter(|_| !orphaned) else {
        for gone in tree.remove(&job.dir) {
            update.listings.insert(gone, None);
        }
        return;
    };
    let mut gone: Vec<Arc<str>> = Vec::new();
    let mut deep = job.deep;
    if let Some(old) = tree.dirs.get(&job.dir) {
        // Its own `.gitignore` changed: what's under it is matched anew.
        deep |= old.rules.own(&job.dir) != rules.own(&job.dir);
        for folder in old.entries.iter().filter(|entry| entry.kind == Kind::Dir) {
            let kept = find(&entries, &folder.name).is_some_and(|entry| entry.kind == Kind::Dir);
            if !kept {
                gone.push(join(&job.dir, &folder.name));
            }
        }
    }
    for folder in entries.iter().filter(|entry| entry.kind == Kind::Dir) {
        let path = join(&job.dir, &folder.name);
        let listed = tree.dirs.get(&path).map(|dir| dir.ignored);
        if folder.ignored && !tree.loaded.contains(&path) {
            if listed.is_some() {
                gone.push(path);
            }
            continue;
        }
        let read = match listed {
            None => true,
            Some(was_ignored) => deep || was_ignored != folder.ignored,
        };
        if read {
            next.push(Job {
                before: before(tree, &path),
                dir: path,
                ignored: folder.ignored,
                above: rules.clone(),
                deep: true,
            });
        }
    }
    for dir in gone {
        for removed in tree.remove(&dir) {
            update.listings.insert(removed, None);
        }
    }
    let dir = Dir {
        entries,
        ignored: job.ignored,
        rules,
    };
    let entries = dir.entries.clone();
    if tree.put(job.dir.clone(), dir) {
        update.listings.insert(job.dir, Some(entries));
    }
}
