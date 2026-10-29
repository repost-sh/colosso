//! Paths inside a checkout, relative to its root with `/` between folders, and the set of
//! them written since someone last looked, kept to a bounded size however many there are.

use std::collections::HashSet;
use std::sync::Arc;

/// What was written, created or removed. Bounded: past `most` paths, the folders they're in
/// stand for them, and past `most` folders, everything does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    /// These files and folders, relative to the root.
    Paths(HashSet<Arc<str>>),
    /// Something inside each of these folders; `""` is the root.
    Folders(HashSet<Arc<str>>),
    /// More than is worth naming, or the platform lost track: anything may have changed.
    Everything,
}

impl Default for Written {
    fn default() -> Self {
        Written::Paths(HashSet::new())
    }
}

impl Written {
    pub fn is_empty(&self) -> bool {
        match self {
            Written::Paths(paths) => paths.is_empty(),
            Written::Folders(folders) => folders.is_empty(),
            Written::Everything => false,
        }
    }

    pub(crate) fn insert(&mut self, path: Arc<str>, most: usize) {
        match self {
            Written::Paths(paths) if paths.len() < most || paths.contains(&path) => {
                paths.insert(path);
            }
            Written::Everything => {}
            // Past the cap, the folder it's in stands for it.
            Written::Paths(_) | Written::Folders(_) => self.insert_folder(parent_of(&path), most),
        }
    }

    pub(crate) fn insert_folder(&mut self, folder: Arc<str>, most: usize) {
        if let Written::Paths(paths) = self {
            let folders = paths.drain().map(|path| parent_of(&path)).collect();
            *self = Written::Folders(folders);
        }
        if let Written::Folders(folders) = self {
            folders.insert(folder);
        }
        self.cap(most);
    }

    /// Adds what `other` names.
    pub(crate) fn merge(&mut self, other: &Written, most: usize) {
        match other {
            Written::Paths(paths) => {
                for path in paths {
                    self.insert(path.clone(), most);
                }
            }
            Written::Folders(folders) => {
                for folder in folders {
                    self.insert_folder(folder.clone(), most);
                }
            }
            Written::Everything => *self = Written::Everything,
        }
    }

    /// Keeps only the paths, or folders, `keep` says to.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        match self {
            Written::Paths(paths) | Written::Folders(paths) => paths.retain(|path| keep(path)),
            Written::Everything => {}
        }
    }

    fn cap(&mut self, most: usize) {
        if matches!(self, Written::Folders(folders) if folders.len() > most) {
            *self = Written::Everything;
        }
    }
}

/// The folder `path` is in: `""`, the root, for a path at the top.
pub(crate) fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..at])
}

fn parent_of(path: &str) -> Arc<str> {
    parent(path).into()
}

/// The last part of `path`.
pub(crate) fn name(path: &str) -> &str {
    path.rfind('/').map_or(path, |at| &path[at + 1..])
}

/// `name` inside the folder `dir`.
pub(crate) fn join(dir: &str, name: &str) -> Arc<str> {
    let mut path = String::new();
    join_into(&mut path, dir, name).into()
}

/// `name` inside the folder `dir`, written into `path`, which a caller joining many names
/// keeps rather than allocate each.
pub(crate) fn join_into<'a>(path: &'a mut String, dir: &str, name: &str) -> &'a str {
    path.clear();
    if !dir.is_empty() {
        path.push_str(dir);
        path.push('/');
    }
    path.push_str(name);
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> HashSet<Arc<str>> {
        paths.iter().map(|path| Arc::from(*path)).collect()
    }

    #[test]
    fn paths_split_into_folder_and_name() {
        assert_eq!((parent("a/b/c.rs"), name("a/b/c.rs")), ("a/b", "c.rs"));
        assert_eq!((parent("top.rs"), name("top.rs")), ("", "top.rs"));
        assert_eq!(&*join("", "a"), "a");
        assert_eq!(&*join("a/b", "c"), "a/b/c");
    }

    #[test]
    fn past_the_cap_folders_stand_for_paths_and_then_everything_does() {
        let mut written = Written::default();
        for path in ["a/1", "a/2", "b/1"] {
            written.insert(path.into(), 3);
        }
        assert_eq!(written, Written::Paths(set(&["a/1", "a/2", "b/1"])));
        written.insert("c/x/1".into(), 3);
        assert_eq!(written, Written::Folders(set(&["a", "b", "c/x"])));
        written.insert("a/3".into(), 3);
        assert_eq!(written, Written::Folders(set(&["a", "b", "c/x"])));
        written.insert("d/1".into(), 3);
        assert_eq!(written, Written::Everything);
        written.insert("e/1".into(), 3);
        assert_eq!(written, Written::Everything);
    }

    #[test]
    fn merging_keeps_the_bound() {
        let mut written = Written::Paths(set(&["a/1"]));
        written.merge(&Written::Folders(set(&["b"])), 10);
        assert_eq!(written, Written::Folders(set(&["a", "b"])));
        written.merge(&Written::Everything, 10);
        assert_eq!(written, Written::Everything);
        assert!(!written.is_empty());
        assert!(Written::default().is_empty());
    }
}
