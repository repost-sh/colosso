//! Every folder listed so far, by path. It answers what the watcher asks of each write
//! (whether it can matter) and what subscribers are told of it (whether git ignores it).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::entry::{Entry, Kind, find};
use crate::ignores::Ignores;
use crate::paths::{join, name, parent};

/// A folder as last listed.
#[derive(Debug)]
pub(crate) struct Dir {
    pub entries: Arc<[Entry]>,
    pub ignored: bool,
    /// The rules its entries were matched with; the rules it was read under are its
    /// parent's.
    pub rules: Arc<Ignores>,
}

impl Dir {
    /// How many of its entries are files git doesn't ignore.
    fn files(&self) -> usize {
        (self.entries.iter())
            .filter(|entry| !entry.is_folder() && !entry.ignored)
            .count()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Tree {
    pub dirs: HashMap<Arc<str>, Dir>,
    /// Ignored folders read because a consumer asked, kept listed as they change.
    pub loaded: HashSet<Arc<str>>,
    /// Bumped whenever a listing changes, so a list made from them knows when it's out of date.
    pub generation: u64,
    /// How many files are listed that git doesn't ignore.
    pub files: usize,
}

impl Tree {
    /// Files `new` as the listing of `dir`; whether that changed what's listed. A listing
    /// read again the same keeps the one before, so what's made from it can tell it's the same.
    pub(crate) fn put(&mut self, dir: Arc<str>, mut new: Dir) -> bool {
        self.files += new.files();
        let changed = match self.dirs.get(&dir) {
            Some(old) => {
                self.files -= old.files();
                let changed = old.entries != new.entries || old.ignored != new.ignored;
                if !changed {
                    new.entries = old.entries.clone();
                }
                changed
            }
            None => true,
        };
        self.dirs.insert(dir, new);
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Takes `dir` and every folder listed under it out; the folders taken.
    pub(crate) fn remove(&mut self, dir: &str) -> Vec<Arc<str>> {
        let mut removed = Vec::new();
        let mut under: Vec<Arc<str>> = vec![dir.into()];
        while let Some(dir) = under.pop() {
            self.loaded.remove(&dir);
            if let Some(gone) = self.dirs.remove(&dir) {
                self.files -= gone.files();
                let folders = gone.entries.iter().filter(|entry| entry.kind == Kind::Dir);
                under.extend(folders.map(|entry| join(&dir, &entry.name)));
                removed.push(dir);
            }
        }
        if !removed.is_empty() {
            self.generation += 1;
        }
        removed
    }

    /// The nearest folder listed above `path`, by its path, and the part of `path` directly
    /// in it; `None` if nothing above is listed yet.
    fn nearest<'a>(&self, path: &'a str) -> Option<(&Arc<str>, &Dir, &'a str)> {
        let (mut below, mut above) = (path, parent(path));
        loop {
            if let Some((dir, listed)) = self.dirs.get_key_value(above) {
                return Some((dir, listed, below));
            }
            if above.is_empty() {
                return None;
            }
            (below, above) = (above, parent(above));
        }
    }

    /// Whether a write at `path` can change anything listed: not inside a folder that git
    /// ignores and nobody has asked to read. Asked for each write as the platform reports it.
    pub(crate) fn admits(&self, path: &str) -> bool {
        // Nothing above it listed: the first scan hasn't got that far.
        let Some((_, dir, below)) = self.nearest(path) else {
            return true;
        };
        // Directly in a listed folder, whose listing it may change; or inside `below`, a
        // folder that isn't listed, read only if it isn't ignored.
        below == path
            || !dir.ignored
                && match find(&dir.entries, name(below)) {
                    Some(entry) => !entry.ignored,
                    None => !dir.rules.ignores(below, true),
                }
    }

    /// Whether git ignores `path`, as far as the listings say: a path that's gone is matched
    /// against the rules of the folder it was in.
    pub(crate) fn ignores(&self, path: &str) -> bool {
        if let Some(dir) = self.dirs.get(path) {
            return dir.ignored;
        }
        self.nearest(path).is_some_and(|(_, dir, below)| {
            dir.ignored
                || match find(&dir.entries, name(below)) {
                    Some(entry) => entry.ignored,
                    None => dir.rules.ignores(below, below != path),
                }
        })
    }

    /// The folder to read again for a write directly in `folder`: `folder` itself if it's
    /// listed, or else the nearest folder listed above it, which the new folders are in.
    /// `None` inside a folder git ignores that nobody asked to read.
    pub(crate) fn nearest_listed(&self, folder: &str) -> Option<Arc<str>> {
        if let Some((dir, _)) = self.dirs.get_key_value(folder) {
            return Some(dir.clone());
        }
        if self.ignores(folder) {
            return None;
        }
        self.nearest(folder).map(|(dir, _, _)| dir.clone())
    }

    /// The rules `dir` is read under: its parent's, or `base` for the root. `None` if its
    /// parent isn't listed.
    pub(crate) fn above(&self, dir: &str, base: &Arc<Ignores>) -> Option<Arc<Ignores>> {
        if dir.is_empty() {
            return Some(base.clone());
        }
        self.dirs
            .get(parent(dir))
            .map(|listed| listed.rules.clone())
    }

    /// The listings of folders git doesn't ignore, as they are now, to make the file list
    /// from once the tree is let go.
    pub(crate) fn listings(&self) -> Vec<(Arc<str>, Arc<[Entry]>)> {
        (self.dirs.iter())
            .filter(|(_, dir)| !dir.ignored)
            .map(|(path, dir)| (path.clone(), dir.entries.clone()))
            .collect()
    }
}
