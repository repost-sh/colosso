//! What to read again once the watcher reports writes, and reading it: the files touched,
//! new, or changed in status, while every other file keeps the diff it had.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use colosso_git::{Change, Checkout, Status};

use super::file_view::{FileView, Key, key};
use super::watch::Touched;

/// What has to be read again.
#[derive(Debug, Default)]
pub struct Stale {
    files: HashSet<String>,
    /// Folders with too much written in them to name file by file: every file in them is
    /// stale. The project keeps how many there are to a bound.
    folders: Vec<String>,
    /// Whether the index changed, which is what staged changes, and the unstaged ones beside
    /// them, are diffed from.
    index: bool,
    everything: bool,
}

impl Stale {
    /// Everything, as before the first read.
    pub fn everything() -> Self {
        Stale {
            everything: true,
            ..Stale::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.everything && !self.index && self.files.is_empty() && self.folders.is_empty()
    }

    pub fn add(&mut self, touched: Vec<Touched>) {
        for touched in touched {
            match touched {
                Touched::File(path) => {
                    self.files.insert(path);
                }
                Touched::Folder(folder) if folder.is_empty() => self.everything = true,
                Touched::Folder(folder) => self.folders.push(folder),
                Touched::Index => self.index = true,
                Touched::Head | Touched::Everything => self.everything = true,
            }
        }
    }

    /// Whether a write at `path` is stale: written itself, or in a folder written in.
    fn touched(&self, path: &str) -> bool {
        self.files.contains(path)
            || (self.folders.iter()).any(|folder| {
                path.strip_prefix(folder.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    }

    /// Whether `change` is stale; with the index changed, one of `staged`'s paths is.
    fn has(&self, change: &Change, staged: &HashSet<&str>) -> bool {
        let touched = |path: &str| self.touched(path);
        self.everything
            || (self.index && staged.contains(change.path.as_str()))
            // Writing a file changes only what's unstaged of it.
            || !change.staged
                && (touched(&change.path)
                    || matches!(&change.status, Status::Renamed { from } if touched(from)))
    }
}

/// What a read found, to put in place of what was shown.
pub struct Read {
    pub listed: Vec<Change>,
    pub files: Vec<Arc<FileView>>,
}

/// Lists the changes again and diffs the files that are new, changed in status, or `stale`,
/// keeping the rest of `files` as they are.
pub fn read(
    checkout: &Checkout,
    stale: &Stale,
    listed: &[Change],
    files: Vec<Arc<FileView>>,
) -> colosso_git::Result<Read> {
    let started = Instant::now();
    let changes = checkout.changes()?;
    let unread = unread(&changes, listed, stale);
    let reread: HashSet<Key> = unread.iter().map(key).collect();
    let diffs = checkout.diffs(&unread);
    log::debug!(
        "listed {} changes and diffed {} files in {:.1?}",
        changes.len(),
        diffs.len(),
        started.elapsed()
    );
    let read = diffs
        .into_iter()
        .map(|diff| Arc::new(FileView::new(diff)))
        .collect();
    Ok(Read {
        files: merged(&changes, files, &reread, read),
        listed: changes,
    })
}

/// Which of `changes` to read again: the `stale` ones and any `listed` differently before.
fn unread(changes: &[Change], listed: &[Change], stale: &Stale) -> Vec<Change> {
    // Staging a file moves what its unstaged change is diffed from, so with the index changed,
    // every file staged before or now is read again.
    let staged: HashSet<&str> = (listed.iter().chain(changes))
        .filter(|change| change.staged)
        .map(|change| change.path.as_str())
        .collect();
    let listed: HashSet<&Change> = listed.iter().collect();
    changes
        .iter()
        .filter(|change| stale.has(change, &staged) || !listed.contains(change))
        .cloned()
        .collect()
}

/// The files for `changes`, in their order: those `read`, and the rest of `shown` as they were.
/// A file `reread` that isn't in `read` has no change left on its side, so has no diff.
fn merged(
    changes: &[Change],
    shown: Vec<Arc<FileView>>,
    reread: &HashSet<Key>,
    read: Vec<Arc<FileView>>,
) -> Vec<Arc<FileView>> {
    let mut files: HashMap<Key, Arc<FileView>> = shown
        .into_iter()
        .filter(|file| !reread.contains(&key(&file.diff.change)))
        .chain(read)
        .map(|file| (key(&file.diff.change), file))
        .collect();
    changes
        .iter()
        .filter_map(|change| files.remove(&key(change)))
        .collect()
}

#[cfg(test)]
mod tests {
    use colosso_git::{Content, FileDiff};

    use super::*;

    fn change(path: &str, status: Status) -> Change {
        Change {
            path: path.into(),
            status,
            staged: false,
        }
    }

    fn file(change: &Change) -> Arc<FileView> {
        Arc::new(FileView::new(FileDiff {
            change: change.clone(),
            content: Content::Binary,
        }))
    }

    fn stale(files: &[&str]) -> Stale {
        let mut stale = Stale::default();
        stale.add(
            files
                .iter()
                .map(|path| Touched::File((*path).into()))
                .collect(),
        );
        stale
    }

    #[test]
    fn only_touched_new_and_restatused_files_are_read_again() {
        let a = change("a.rs", Status::Modified);
        let b = change("b.rs", Status::Modified);
        let c = change("c.rs", Status::Added);
        let listed = vec![a.clone(), b.clone(), change("c.rs", Status::Modified)];
        let changes = [a, b, c, change("d.rs", Status::Added)];
        let unread = unread(&changes, &listed, &stale(&["a.rs"]));
        let paths: Vec<&str> = unread.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["a.rs", "c.rs", "d.rs"]);
    }

    #[test]
    fn a_rename_is_read_again_when_its_source_is_touched() {
        let moved = change(
            "new.rs",
            Status::Renamed {
                from: "old.rs".into(),
            },
        );
        let unread = unread(
            std::slice::from_ref(&moved),
            std::slice::from_ref(&moved),
            &stale(&["old.rs"]),
        );
        assert_eq!(unread.len(), 1);
    }

    #[test]
    fn head_moving_or_the_root_flooded_reads_everything_again() {
        let a = change("a.rs", Status::Modified);
        let mut head = Stale::default();
        head.add(vec![Touched::Index, Touched::Head]);
        assert_eq!(
            unread(std::slice::from_ref(&a), std::slice::from_ref(&a), &head).len(),
            1
        );
        let mut root = Stale::default();
        root.add(vec![Touched::Folder(String::new())]);
        assert!(root.everything);
    }

    #[test]
    fn a_flooded_folder_reads_again_what_is_inside_it() {
        let changes = [
            change("src/a.rs", Status::Modified),
            change("src/deep/b.rs", Status::Modified),
            change("srcs/c.rs", Status::Modified),
            change("d.rs", Status::Modified),
        ];
        let mut flooded = Stale::default();
        flooded.add(vec![Touched::Folder("src".into())]);
        let unread = unread(&changes, &changes, &flooded);
        let paths: Vec<&str> = unread.iter().map(|change| change.path.as_str()).collect();
        assert_eq!(paths, ["src/a.rs", "src/deep/b.rs"]);
    }

    #[test]
    fn the_index_moving_reads_again_what_is_or_was_staged() {
        let staged = |path: &str| Change {
            staged: true,
            ..change(path, Status::Modified)
        };
        let unstaged = |path: &str| change(path, Status::Modified);
        // `b.rs` was staged and now isn't, so its unstaged change has a new base.
        let listed = vec![
            staged("a.rs"),
            staged("b.rs"),
            unstaged("a.rs"),
            unstaged("b.rs"),
            unstaged("c.rs"),
        ];
        let changes = [
            staged("a.rs"),
            unstaged("a.rs"),
            unstaged("b.rs"),
            unstaged("c.rs"),
        ];
        let mut index = Stale::default();
        index.add(vec![Touched::Index]);
        assert_eq!(unread(&changes, &listed, &index), changes[..3]);
        // Writing a file leaves what's staged of it as it was.
        assert_eq!(
            unread(&changes, &listed, &stale(&["a.rs"])),
            [unstaged("a.rs")]
        );
    }

    #[test]
    fn files_read_again_replace_theirs_and_the_rest_are_kept_in_order() {
        let a = change("a.rs", Status::Modified);
        let b = change("b.rs", Status::Modified);
        let undone = change("undone.rs", Status::Modified);
        let shown = vec![file(&a), file(&b), file(&undone)];
        let kept = shown[1].clone();
        let new_a = file(&a);
        let reread: HashSet<Key> = [key(&a), key(&undone)].into();
        let files = merged(
            &[b, a, undone, change("gone-from-status.rs", Status::Added)],
            shown,
            &reread,
            vec![new_a.clone()],
        );
        assert_eq!(files.len(), 2);
        assert!(Arc::ptr_eq(&files[0], &kept));
        assert!(Arc::ptr_eq(&files[1], &new_a));
    }
}
