//! The file tree as a list: the folders a consumer has opened, and the rows they show,
//! flattened top to bottom in the order they're drawn. Opening a folder, closing it and a
//! new listing each replace only the rows of the folder they touch, so a checkout of half a
//! million files costs what's open and what changed, never the whole tree.
//!
//! The rows are in tree order, which the paths alone decide, so the row showing a path is
//! found by bisecting them: no index of rows by path is kept, and none has to be shifted
//! when rows come and go.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::Project;
use crate::entry::{Entry, Kind, by_place};
use crate::paths::{join_into, parent};

/// Where the rows' listings come from: a project, or anything that lists folders as one does.
pub trait Listings {
    /// The listing of `dir`, if it's been read.
    fn listing(&self, dir: &str) -> Option<Arc<[Entry]>>;
    /// Asks for the ignored folder `dir` to be read; its listing comes in a later update.
    fn load(&self, dir: &str);
}

impl Listings for Project {
    fn listing(&self, dir: &str) -> Option<Arc<[Entry]>> {
        Project::listing(self, dir)
    }

    fn load(&self, dir: &str) {
        Project::load(self, dir);
    }
}

/// One file or folder as the tree shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// How many folders it's in below the root: 0 at the top.
    pub depth: u32,
    pub entry: Entry,
    /// Relative to the root, with `/` between folders.
    pub path: Arc<str>,
    /// Whether it's a folder that's open, with its rows after it.
    pub expanded: bool,
}

/// Where revealing a path left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal {
    /// Shown, at this row.
    Shown(usize),
    /// Not yet: a folder above it isn't read yet, and is asked for if it's ignored.
    Pending,
    /// Not there: every folder above it is read, and it isn't in them.
    Missing,
}

/// The rows shown, and the folders open. A folder closed keeps the folders inside it open,
/// so opening it again shows them as they were.
#[derive(Debug, Default)]
pub struct Rows {
    rows: Vec<Row>,
    expanded: HashSet<Arc<str>>,
}

impl Rows {
    /// Every row, top to bottom.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    /// The row showing `path`, if it's shown.
    pub fn position(&self, path: &str) -> Option<usize> {
        if path.is_empty() {
            return None;
        }
        // Whether it's a folder decides its place among its siblings; it's tried as each.
        [true, false].into_iter().find_map(|folder| {
            (self.rows)
                .binary_search_by(|row| {
                    tree_order((&row.path, row.entry.is_folder()), (path, folder))
                })
                .ok()
        })
    }

    /// The row of the folder the `index`th row is in; `None` for a row at the top.
    pub fn parent(&self, index: usize) -> Option<usize> {
        self.position(parent(&self.rows.get(index)?.path))
    }

    /// Makes every row again from `listings`, keeping which folders are open: for the
    /// first listings, or when a consumer can't tell what changed.
    pub fn reset(&mut self, listings: &impl Listings) {
        let mut rows = Vec::new();
        self.children("", false, 0, listings, &mut rows);
        self.rows = rows;
    }

    /// Opens the folder `dir`, if it's shown and closed; whether it was. An ignored folder
    /// not read yet is asked for, and its rows come with its listing.
    pub fn expand(&mut self, dir: &str, listings: &impl Listings) -> bool {
        let Some(at) = self.position(dir) else {
            return false;
        };
        let row = &self.rows[at];
        if row.expanded || row.entry.kind != Kind::Dir {
            return false;
        }
        self.expanded.insert(row.path.clone());
        self.open(at, listings);
        true
    }

    /// Closes the folder `dir`, if it's shown and open; whether it was.
    pub fn collapse(&mut self, dir: &str) -> bool {
        let Some(at) = self.position(dir) else {
            return false;
        };
        if !self.rows[at].expanded {
            return false;
        }
        self.expanded.remove(dir);
        let end = self.end_of(Some(at));
        self.rows[at].expanded = false;
        self.rows.drain(at + 1..end);
        true
    }

    /// Takes the listings an update brought, each in place of the one before (`None` for a
    /// folder gone); whether any row changed. Only open folders that are shown have rows to
    /// change, and only the entries that differ are made again.
    pub fn apply(
        &mut self,
        changed: &HashMap<Arc<str>, Option<Arc<[Entry]>>>,
        listings: &impl Listings,
    ) -> bool {
        let mut dirs: Vec<_> = changed.iter().collect();
        // Folders above first, so each finds its own row as its parent's listing left it.
        dirs.sort_by_key(|(dir, _)| depth_of(dir));
        let mut any = false;
        for (dir, entries) in dirs {
            any |= self.relist(dir, entries.as_deref().unwrap_or(&[]), listings);
        }
        any
    }

    /// Opens every folder above `path`, so it's shown, and says where it is. An ignored
    /// folder on the way that's not been read is asked for: the path is shown once its
    /// listing comes, and revealing it again then finds it.
    pub fn reveal(&mut self, path: &str, listings: &impl Listings) -> Reveal {
        // The highest folder that's closed: those above it are open, so it's shown, and
        // opening it shows the rest in one go.
        let mut closed = None;
        for dir in path.match_indices('/').map(|(at, _)| &path[..at]) {
            if !self.expanded.contains(dir) {
                self.expanded.insert(dir.into());
                closed = closed.or(Some(dir));
            }
        }
        if let Some(at) = closed.and_then(|dir| self.position(dir)) {
            self.open(at, listings);
        }
        if let Some(at) = self.position(path) {
            return Reveal::Shown(at);
        }
        // Not shown: either a folder on the way isn't read yet, or one isn't there.
        let mut dir = "";
        for part in path.split('/') {
            let Some(entries) = listings.listing(dir) else {
                return Reveal::Pending;
            };
            if !entries.iter().any(|entry| *entry.name == *part) {
                return Reveal::Missing;
            }
            dir = &path[..dir.len() + usize::from(!dir.is_empty()) + part.len()];
        }
        Reveal::Missing
    }

    /// Shows the folder at `at` open, with the rows under it, through every open folder
    /// inside it.
    fn open(&mut self, at: usize, listings: &impl Listings) {
        let row = &self.rows[at];
        let (dir, ignored, depth) = (row.path.clone(), row.entry.ignored, row.depth);
        let mut rows = Vec::new();
        self.children(&dir, ignored, depth + 1, listings, &mut rows);
        self.rows[at].expanded = true;
        let after = at + 1;
        self.rows.splice(after..after, rows);
    }

    /// Puts the rows under the folder `dir` in `rows`, `depth` deep, through every open
    /// folder inside it.
    fn children(
        &self,
        dir: &str,
        ignored: bool,
        depth: u32,
        listings: &impl Listings,
        rows: &mut Vec<Row>,
    ) {
        match listings.listing(dir) {
            Some(entries) => {
                rows.reserve(entries.len());
                let mut path = String::new();
                for entry in entries.iter() {
                    let path = Arc::from(join_into(&mut path, dir, &entry.name));
                    self.push(path, entry, depth, listings, rows);
                }
            }
            None if ignored => listings.load(dir),
            // Not read yet: the first scan brings it.
            None => {}
        }
    }

    /// Puts `entry`, at `path`, in `rows`, and what's under it if it's open.
    fn push(
        &self,
        path: Arc<str>,
        entry: &Entry,
        depth: u32,
        listings: &impl Listings,
        rows: &mut Vec<Row>,
    ) {
        let expanded = entry.kind == Kind::Dir && self.expanded.contains(&path);
        let open = expanded.then(|| path.clone());
        rows.push(Row {
            depth,
            entry: entry.clone(),
            path,
            expanded,
        });
        if let Some(dir) = open {
            self.children(&dir, entry.ignored, depth + 1, listings, rows);
        }
    }

    /// Where the rows under the folder at `at` end: past the last row, for the root.
    fn end_of(&self, at: Option<usize>) -> usize {
        let Some(at) = at else {
            return self.rows.len();
        };
        let dir = &self.rows[at].path;
        // What's under a folder follows it, all together.
        at + 1 + self.rows[at + 1..].partition_point(|row| is_under(&row.path, dir))
    }

    /// Shows `entries` as the listing of `dir`, if it's open and shown; whether a row
    /// changed. Entries the same at either end keep their rows, and those of the folders
    /// open under them; the ones between are made again, in one splice.
    fn relist(&mut self, dir: &str, entries: &[Entry], listings: &impl Listings) -> bool {
        let at = if dir.is_empty() {
            None
        } else {
            match self.position(dir) {
                Some(at) if self.rows[at].expanded => Some(at),
                _ => return false,
            }
        };
        let depth = at.map_or(0, |at| self.rows[at].depth + 1);
        let end = self.end_of(at);
        // Where each of the entries shown now starts.
        let mut shown = Vec::new();
        let mut index = at.map_or(0, |at| at + 1);
        while index < end {
            shown.push(index);
            index = if self.rows[index].expanded {
                self.end_of(Some(index))
            } else {
                index + 1
            };
        }
        let same = |(&index, entry): (&usize, &Entry)| self.rows[index].entry == *entry;
        let first = shown
            .iter()
            .zip(entries)
            .take_while(|&pair| same(pair))
            .count();
        let last = (shown[first..].iter().rev())
            .zip(entries[first..].iter().rev())
            .take_while(|&pair| same(pair))
            .count();
        if first + last == shown.len() && shown.len() == entries.len() {
            return false;
        }
        let from = shown.get(first).copied().unwrap_or(end);
        let to = shown.get(shown.len() - last).copied().unwrap_or(end);
        let mut rows = Vec::new();
        let mut path = String::new();
        for entry in &entries[first..entries.len() - last] {
            let path = Arc::from(join_into(&mut path, dir, &entry.name));
            self.push(path, entry, depth, listings, &mut rows);
        }
        self.rows.splice(from..to, rows);
        true
    }
}

/// How many folders deep `dir` is: 0 for the root.
fn depth_of(dir: &str) -> usize {
    if dir.is_empty() {
        0
    } else {
        dir.matches('/').count() + 1
    }
}

/// Whether `path` is inside the folder `dir`.
fn is_under(path: &str, dir: &str) -> bool {
    path.as_bytes().get(dir.len()) == Some(&b'/') && path.starts_with(dir)
}

/// Two paths, each with whether it's a folder, in the order the tree shows them: a folder
/// before what's in it, and within a folder by its listing's order.
fn tree_order((one, one_folder): (&str, bool), (other, other_folder): (&str, bool)) -> Ordering {
    let (mut ones, mut others) = (one.split('/'), other.split('/'));
    loop {
        match (ones.next(), others.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(part), Some(other_part)) if part == other_part => {}
            (Some(part), Some(other_part)) => {
                // A part with more after it is a folder; the last is whatever the path is.
                let folder = one_folder || ones.next().is_some();
                let other_is_folder = other_folder || others.next().is_some();
                return by_place((folder, part), (other_is_folder, other_part));
            }
        }
    }
}

#[cfg(test)]
mod tests;
