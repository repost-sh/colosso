//! The list of every file git doesn't ignore, by path, for a fuzzy finder. It's made from the
//! listings once they're let go, and made again only for the folders whose listing changed:
//! the paths of the rest are kept from the last time, so a consumer that holds on to a
//! folder's paths can tell it's unchanged by the pointer.

use std::collections::HashMap;
use std::sync::Arc;

use crate::entry::Entry;
use crate::paths::join;

/// Paths, shared.
type Paths = Arc<[Arc<str>]>;

/// One folder's files that git doesn't ignore, by path, in the folder's listing order.
#[derive(Clone, Debug)]
pub struct Folder {
    /// The folder, relative to the checkout; `""` is the root.
    pub path: Arc<str>,
    /// Never empty; the same `Arc` from one call to the next until the listing changes.
    pub files: Paths,
}

impl Folder {
    /// The folder at `path` with the files `entries`, its listing, has that git doesn't
    /// ignore; `None` if it has none.
    pub fn of(path: Arc<str>, entries: &[Entry]) -> Option<Folder> {
        let files = paths(&path, entries);
        (!files.is_empty()).then_some(Folder { path, files })
    }
}

/// The paths of the files in `entries`, the listing of `dir`, that git doesn't ignore.
fn paths(dir: &str, entries: &[Entry]) -> Paths {
    (entries.iter())
        .filter(|entry| !entry.is_folder() && !entry.ignored)
        .map(|entry| join(dir, &entry.name))
        .collect()
}

#[derive(Default)]
pub(crate) struct Files {
    /// The tree's generation `by_dir` was made at.
    generation: u64,
    /// Each folder's files by path, with the listing they were made from.
    by_dir: HashMap<Arc<str>, (Arc<[Entry]>, Paths)>,
    /// What was asked for at this generation, made the first time it was.
    list: Option<Paths>,
    folders: Option<Arc<[Folder]>>,
}

impl Files {
    pub(crate) fn is_at(&self, generation: u64) -> bool {
        self.generation == generation
    }

    /// Takes up `listings`, the tree's at `generation`.
    pub(crate) fn update(&mut self, generation: u64, listings: Vec<(Arc<str>, Arc<[Entry]>)>) {
        let mut by_dir = HashMap::with_capacity(listings.len());
        for (dir, entries) in listings {
            // A listing that hasn't changed is the very one the paths were made from.
            let paths = match self.by_dir.remove(&dir) {
                Some((made_from, paths)) if Arc::ptr_eq(&made_from, &entries) => paths,
                _ => paths(&dir, &entries),
            };
            by_dir.insert(dir, (entries, paths));
        }
        self.by_dir = by_dir;
        self.generation = generation;
        self.list = None;
        self.folders = None;
    }

    /// Every file, in no particular order.
    pub(crate) fn list(&mut self) -> Paths {
        let by_dir = &self.by_dir;
        (self.list.get_or_insert_with(|| {
            (by_dir.values())
                .flat_map(|(_, paths)| paths.iter().cloned())
                .collect()
        }))
        .clone()
    }

    /// Every folder with files in it, in no particular order.
    pub(crate) fn folders(&mut self) -> Arc<[Folder]> {
        let by_dir = &self.by_dir;
        (self.folders.get_or_insert_with(|| {
            (by_dir.iter())
                .filter(|(_, (_, paths))| !paths.is_empty())
                .map(|(dir, (_, paths))| Folder {
                    path: dir.clone(),
                    files: paths.clone(),
                })
                .collect()
        }))
        .clone()
    }
}
