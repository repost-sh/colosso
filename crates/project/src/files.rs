//! The list of every file git doesn't ignore, by path, for a fuzzy finder. It's made from the
//! listings once they're let go, and made again only for the folders whose listing changed:
//! the paths of the rest are kept from the last time.

use std::collections::HashMap;
use std::sync::Arc;

use crate::entry::Entry;
use crate::paths::join;

/// Paths, shared.
type Paths = Arc<[Arc<str>]>;

#[derive(Default)]
pub(crate) struct Files {
    /// The tree's generation the list was made at.
    generation: u64,
    list: Paths,
    /// Each folder's files by path, with the listing they were made from.
    by_dir: HashMap<Arc<str>, (Arc<[Entry]>, Paths)>,
}

impl Files {
    /// The list as it was made at `generation`, if it was.
    pub(crate) fn at(&self, generation: u64) -> Option<Paths> {
        (self.generation == generation).then(|| self.list.clone())
    }

    /// The list for `listings`, the tree's at `generation`.
    pub(crate) fn make(
        &mut self,
        generation: u64,
        listings: Vec<(Arc<str>, Arc<[Entry]>)>,
    ) -> Paths {
        let mut by_dir = HashMap::with_capacity(listings.len());
        for (dir, entries) in listings {
            // A listing that hasn't changed is the very one the paths were made from.
            let paths = match self.by_dir.remove(&dir) {
                Some((made_from, paths)) if Arc::ptr_eq(&made_from, &entries) => paths,
                _ => (entries.iter())
                    .filter(|entry| !entry.is_folder() && !entry.ignored)
                    .map(|entry| join(&dir, &entry.name))
                    .collect(),
            };
            by_dir.insert(dir, (entries, paths));
        }
        self.by_dir = by_dir;
        self.list = (self.by_dir.values())
            .flat_map(|(_, paths)| paths.iter().cloned())
            .collect();
        self.generation = generation;
        self.list.clone()
    }
}
