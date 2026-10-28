//! Paths as a tree of folders, flattened into rows for a list. A folder holding nothing but one
//! other folder shares its row, `src/changes`, so deep paths don't push names off the panel.

use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Folder {
        /// The folder's path from the top, which closing it is remembered by.
        path: String,
        /// The folder's name, with the names of any folders it merged with.
        name: String,
        depth: usize,
    },
    /// The `index`th of the paths given.
    File { index: usize, depth: usize },
}

#[derive(Default)]
struct Folder<'a> {
    folders: BTreeMap<&'a str, Folder<'a>>,
    files: Vec<(&'a str, usize)>,
}

/// The rows for `paths`, folders before files and each by name, leaving out what's in the
/// folders `closed`.
pub fn entries<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    closed: &HashSet<String>,
) -> Vec<Entry> {
    let mut top = Folder::default();
    for (index, path) in paths.into_iter().enumerate() {
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap_or(path);
        let folder = parts.into_iter().fold(&mut top, |folder, part| {
            folder.folders.entry(part).or_default()
        });
        folder.files.push((name, index));
    }
    let mut entries = Vec::new();
    flatten(&top, "", 0, closed, &mut entries);
    entries
}

fn flatten(
    folder: &Folder,
    path: &str,
    depth: usize,
    closed: &HashSet<String>,
    entries: &mut Vec<Entry>,
) {
    for (name, mut inner) in &folder.folders {
        let mut name = (*name).to_owned();
        while inner.files.is_empty() && inner.folders.len() == 1 {
            let (next, only) = inner.folders.iter().next().expect("one folder");
            name = format!("{name}/{next}");
            inner = only;
        }
        let path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}/{name}")
        };
        let open = !closed.contains(&path);
        entries.push(Entry::Folder {
            path: path.clone(),
            name,
            depth,
        });
        if open {
            flatten(inner, &path, depth + 1, closed, entries);
        }
    }
    let mut files = folder.files.clone();
    files.sort_unstable();
    entries.extend(
        files
            .into_iter()
            .map(|(_, index)| Entry::File { index, depth }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, name: &str, depth: usize) -> Entry {
        Entry::Folder {
            path: path.into(),
            name: name.into(),
            depth,
        }
    }

    fn file(index: usize, depth: usize) -> Entry {
        Entry::File { index, depth }
    }

    #[test]
    fn folders_come_first_and_lone_folders_share_a_row() {
        let paths = [
            "build.rs",
            "crates/app/src/main.rs",
            "crates/app/src/lib.rs",
            "README.md",
        ];
        assert_eq!(
            entries(paths, &HashSet::new()),
            [
                folder("crates/app/src", "crates/app/src", 0),
                file(2, 1),
                file(1, 1),
                // By byte, as git sorts: capitals first.
                file(3, 0),
                file(0, 0),
            ]
        );
    }

    #[test]
    fn a_folder_with_files_of_its_own_keeps_its_row() {
        let paths = ["src/lib.rs", "src/changes/rows.rs", "src/changes/tree.rs"];
        assert_eq!(
            entries(paths, &HashSet::new()),
            [
                folder("src", "src", 0),
                folder("src/changes", "changes", 1),
                file(1, 2),
                file(2, 2),
                file(0, 1),
            ]
        );
    }

    #[test]
    fn a_closed_folder_hides_what_it_holds() {
        let paths = ["src/lib.rs", "src/changes/rows.rs"];
        let closed = HashSet::from(["src/changes".to_owned()]);
        assert_eq!(
            entries(paths, &closed),
            [
                folder("src", "src", 0),
                folder("src/changes", "changes", 1),
                file(0, 1)
            ]
        );
    }
}
