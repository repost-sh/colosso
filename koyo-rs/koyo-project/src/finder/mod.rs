//! Go to File's search: every file git doesn't ignore, matched by fuzzy path as fzf matches
//! (nucleo's matcher, scoring paths), best first. Nothing here knows about GPUI; a finder is
//! shared by whoever searches it, locks itself, and runs each search on a pool of threads it
//! makes once.
//!
//! A match within a file's name ranks above any that needs its folders, so `mainrs` finds
//! `src/main.rs` and `chv` still finds `changes/view.rs`. Each search keeps what it matched
//! while the query only grows, so the next keystroke matches those again rather than every
//! file, and erasing returns to what was kept. Files are taken up by folder: a folder whose
//! files didn't change keeps its matches, and only the folders that did are matched again.
//! A search gives way to one asked for after it, so a keystroke never waits on the last.

mod pool;
mod query;
mod rank;
pub use rank::matched_bytes;
mod scan;
#[cfg(test)]
mod tests;

use std::cmp::Ordering;
use std::collections::HashMap;
use std::num::NonZero;
use std::ops::Range;
use std::sync::atomic::{self, AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
pub use query::Query;

use crate::entry::by_place;
use crate::files::Folder;
use crate::lock;
use crate::paths::parent;
use pool::Pool;
use scan::{GaveWay, Hit, name_start};

/// How many searches are kept for the query to be erased back to.
const MOST_KEPT: usize = 8;

/// How many matches Go to File shows: more than anyone scrolls through.
pub const MOST_FOUND: usize = 500;

/// What a search found.
#[derive(Clone, Debug, Default)]
pub struct Results {
    pub query: Query,
    /// The best matches, best first; for no query, the recent files, then the rest in the
    /// file tree's order.
    pub found: Vec<Found>,
    /// How many files matched, of which `found` holds the best; every file for no query.
    pub matched: usize,
    /// How many files there were to search.
    pub files: usize,
    /// What was searched for, to tell what matched in each file found.
    pattern: Option<Arc<Pattern>>,
}

impl Results {
    /// The bytes of the path of the file found at `index` that matched, in order: worked out
    /// when asked, as only the files shown need them.
    pub fn highlights(&self, index: usize) -> Vec<Range<usize>> {
        let (Some(pattern), Some(found)) = (&self.pattern, self.found.get(index)) else {
            return Vec::new();
        };
        let at = (found.name, found.named);
        pool::matching(|matching| rank::highlights(pattern, &found.path, at, matching))
    }
}

/// One file found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Relative to the checkout.
    pub path: Arc<str>,
    /// Where in `path` the file's name starts.
    pub name: usize,
    /// Whether it's among the files opened lately.
    pub recent: bool,
    /// Whether what was typed matched within the name, where its highlights then are.
    named: bool,
}

/// A search kept: what was typed, and the files it matched.
struct Kept {
    text: String,
    pattern: Arc<Pattern>,
    hits: Vec<Hit>,
}

/// One checkout's files, searchable from any thread.
pub struct Finder {
    state: Mutex<State>,
    pool: Pool,
    /// Counts the searches asked for: one under way gives way once a later one is asked for.
    asked: AtomicU64,
    /// Forgetting was asked for while the finder was busy: whoever locks it next forgets.
    forgotten: AtomicBool,
    /// The files opened lately, as last told, for whoever locks the finder next to take up.
    recent: Mutex<Option<Vec<Arc<str>>>>,
}

#[derive(Default)]
struct State {
    /// The folders with files in them, in the order the file tree shows their files.
    folders: Vec<Folder>,
    /// Each folder's place in `folders`, by its path.
    places: HashMap<Arc<str>, u32>,
    files: usize,
    /// The searches since the query last did more than grow, each narrowing the one before.
    kept: Vec<Kept>,
    recent: Vec<Arc<str>>,
    /// Where the recent files are among the folders, the latest first, without those gone.
    recent_at: Vec<(u32, u32)>,
}

impl Default for Finder {
    fn default() -> Self {
        Finder::new()
    }
}

impl Finder {
    /// One that searches on a thread for each the machine runs at once.
    pub fn new() -> Finder {
        Finder::with_threads(std::thread::available_parallelism().map_or(1, NonZero::get))
    }

    /// One that searches on `threads` threads, which it starts now.
    pub fn with_threads(threads: usize) -> Finder {
        Finder {
            state: Mutex::default(),
            pool: Pool::new(threads),
            asked: AtomicU64::new(0),
            forgotten: AtomicBool::new(false),
            recent: Mutex::default(),
        }
    }

    /// How many files it searches.
    pub fn len(&self) -> usize {
        self.state().files
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes up the checkout's files by folder; whether any changed. Only the folders whose
    /// files changed are matched again for the searches kept.
    pub fn set_files(&self, folders: &[Folder]) -> bool {
        self.state().set_files(folders, &self.pool)
    }

    /// The files opened lately, the latest first, for every search from now on; without
    /// waiting for one under way.
    pub fn set_recent(&self, recent: &[Arc<str>]) {
        *lock(&self.recent) = Some(recent.to_vec());
    }

    /// The `most` best matches for what's `typed`; `None` if a later search was asked for
    /// before this one was done.
    pub fn search(&self, typed: &str, most: usize) -> Option<Results> {
        let ticket = self.ask();
        self.search_for(ticket, typed, most)
    }

    /// Lets go of what earlier searches kept to make the next ones quicker, without waiting:
    /// a search under way gives way, and if the finder is busy, whoever locks it next lets go.
    pub fn forget(&self) {
        self.ask();
        match self.state.try_lock() {
            Ok(mut state) => state.kept = Vec::new(),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().kept = Vec::new(),
            Err(TryLockError::WouldBlock) => self.forgotten.store(true, atomic::Ordering::Relaxed),
        }
    }

    /// A ticket for a search, which every search asked for earlier gives way to.
    fn ask(&self) -> u64 {
        self.asked.fetch_add(1, atomic::Ordering::Relaxed) + 1
    }

    fn search_for(&self, ticket: u64, typed: &str, most: usize) -> Option<Results> {
        let gave_way = || self.asked.load(atomic::Ordering::Relaxed) != ticket;
        let mut state = self.state();
        if gave_way() {
            return None;
        }
        state.search(typed, most, &self.pool, &gave_way)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = lock(&self.state);
        if self.forgotten.swap(false, atomic::Ordering::Relaxed) {
            state.kept = Vec::new();
        }
        if let Some(recent) = lock(&self.recent).take() {
            state.recent = recent;
            state.locate_recent();
        }
        state
    }
}

impl State {
    fn set_files(&mut self, folders: &[Folder], pool: &Pool) -> bool {
        // Each folder as it was, if it's unchanged; or the new one, to be matched.
        let mut present = vec![false; self.folders.len()];
        let mut changed: HashMap<u32, &Folder> = HashMap::new();
        let mut added: Vec<Folder> = Vec::new();
        for folder in folders.iter().filter(|folder| !folder.files.is_empty()) {
            match self.places.get(&folder.path) {
                Some(&at) => {
                    present[at as usize] = true;
                    if !Arc::ptr_eq(&self.folders[at as usize].files, &folder.files) {
                        changed.insert(at, folder);
                    }
                }
                None => added.push(folder.clone()),
            }
        }
        let removed = present.iter().filter(|present| !**present).count();
        if changed.is_empty() && added.is_empty() && removed == 0 {
            return false;
        }
        let mut next: Vec<(Option<u32>, Folder)> = (self.folders.iter().enumerate())
            .filter(|(at, _)| present[*at])
            .map(|(at, folder)| match changed.get(&(at as u32)) {
                Some(new) => (None, (*new).clone()),
                None => (Some(at as u32), folder.clone()),
            })
            .collect();
        if added.len() > next.len() / 8 {
            next.extend(added.into_iter().map(|folder| (None, folder)));
            next.sort_by(|(_, a), (_, b)| tree_order(&a.path, &b.path));
        } else {
            for folder in added {
                let at = next.partition_point(|(_, before)| {
                    tree_order(&before.path, &folder.path) == Ordering::Less
                });
                next.insert(at, (None, folder));
            }
        }
        // Where each unchanged folder went, and which folders are to be matched.
        let mut moved = vec![None; self.folders.len()];
        let mut fresh = Vec::new();
        for (at, (was, _)) in next.iter().enumerate() {
            match was {
                Some(was) => moved[*was as usize] = Some(at as u32),
                None => fresh.push(at as u32),
            }
        }
        self.folders = next.into_iter().map(|(_, folder)| folder).collect();
        let folders = &self.folders;
        pool.each(self.kept.iter_mut().collect(), |kept| {
            let unchanged = (kept.hits.iter()).filter_map(|hit| {
                let folder = moved.get(hit.folder as usize).copied().flatten()?;
                Some(Hit { folder, ..*hit })
            });
            let unchanged: Vec<Hit> = unchanged.collect();
            let matched = scan::folders(folders, &fresh, &kept.pattern, pool, &|| false);
            kept.hits = merge(unchanged, matched);
        });
        self.places = (self.folders.iter().enumerate())
            .map(|(at, folder)| (folder.path.clone(), at as u32))
            .collect();
        self.files = self.folders.iter().map(|folder| folder.files.len()).sum();
        self.locate_recent();
        true
    }

    fn locate_recent(&mut self) {
        let mut at = Vec::with_capacity(self.recent.len());
        for path in &self.recent {
            let Some(&folder) = self.places.get(parent(path)) else {
                continue;
            };
            let files = &self.folders[folder as usize].files;
            if let Some(file) = files.iter().position(|file| file == path) {
                let spot = (folder, file as u32);
                if !at.contains(&spot) {
                    at.push(spot);
                }
            }
        }
        self.recent_at = at;
    }

    fn search(
        &mut self,
        typed: &str,
        most: usize,
        pool: &Pool,
        gave_way: GaveWay<'_>,
    ) -> Option<Results> {
        let query = Query::parse(typed);
        let pattern = Pattern::parse(&query.text, CaseMatching::Smart, Normalization::Smart);
        if pattern.atoms.is_empty() {
            return Some(self.listed(query, most));
        }
        while (self.kept.last()).is_some_and(|kept| !query::narrows(&kept.text, &query.text)) {
            self.kept.pop();
        }
        let folders = &self.folders;
        let hits = match self.kept.last() {
            Some(last) if last.text == query.text => None,
            Some(last) => Some(scan::hits(folders, &last.hits, &pattern, pool, gave_way)),
            None => {
                let all: Vec<u32> = (0..folders.len() as u32).collect();
                Some(scan::folders(folders, &all, &pattern, pool, gave_way))
            }
        };
        // What a search that gave way matched may be missing some, so it isn't kept.
        if gave_way() {
            return None;
        }
        if let Some(hits) = hits {
            if self.kept.len() == MOST_KEPT {
                self.kept.remove(0);
            }
            self.kept.push(Kept {
                text: query.text.clone(),
                pattern: Arc::new(pattern),
                hits,
            });
        }
        let last = self.kept.last()?;
        let found = (rank::best(&last.hits, &self.recent_at, most, pool).into_iter())
            .filter_map(|ranked| {
                let folder = self.folders.get(ranked.hit.folder as usize)?;
                Some(Found {
                    path: folder.files.get(ranked.hit.file as usize)?.clone(),
                    name: name_start(folder),
                    recent: ranked.recency != u32::MAX,
                    named: ranked.hit.named(),
                })
            })
            .collect();
        Some(Results {
            query,
            found,
            matched: last.hits.len(),
            files: self.files,
            pattern: Some(last.pattern.clone()),
        })
    }

    /// For no query: the recent files, then the rest in the file tree's order.
    fn listed(&self, query: Query, most: usize) -> Results {
        let found = |(folder, file): (u32, u32), recent: bool| {
            let at = self.folders.get(folder as usize)?;
            Some(Found {
                path: at.files.get(file as usize)?.clone(),
                name: name_start(at),
                recent,
                named: false,
            })
        };
        let recent = (self.recent_at.iter()).filter_map(|spot| found(*spot, true));
        let rest = (self.folders.iter().enumerate())
            .flat_map(|(folder, at)| (0..at.files.len()).map(move |file| (folder, file)))
            .map(|(folder, file)| (folder as u32, file as u32))
            .filter(|spot| !self.recent_at.contains(spot))
            .filter_map(|spot| found(spot, false));
        Results {
            query,
            found: recent.chain(rest).take(most).collect(),
            matched: self.files,
            files: self.files,
            pattern: None,
        }
    }
}

/// Folders in the order the file tree shows their files: a folder's own files come where a
/// file in it would, after its subfolders, as `by_place` orders a listing.
fn tree_order(a: &str, b: &str) -> Ordering {
    /// Each folder on the way, then a nameless file standing for the folder's own.
    fn parts(path: &str) -> impl Iterator<Item = (bool, &str)> {
        (path.split('/').filter(|part| !part.is_empty()))
            .map(|folder| (true, folder))
            .chain([(false, "")])
    }
    let (mut a, mut b) = (parts(a), parts(b));
    while let (Some(x), Some(y)) = (a.next(), b.next()) {
        match by_place(x, y) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
    }
    Ordering::Equal
}

/// Two lists of hits in the folders' order, as one.
fn merge(a: Vec<Hit>, b: Vec<Hit>) -> Vec<Hit> {
    if b.is_empty() {
        return a;
    }
    let mut merged = Vec::with_capacity(a.len() + b.len());
    let (mut a, mut b) = (a.into_iter().peekable(), b.into_iter().peekable());
    while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
        let next = if x.spot() <= y.spot() {
            a.next()
        } else {
            b.next()
        };
        merged.extend(next);
    }
    merged.extend(a);
    merged.extend(b);
    merged
}
