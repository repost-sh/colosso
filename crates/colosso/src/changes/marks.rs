//! What the file tree tints changed files and folders with, from what git status listed:
//! each changed file's status, staged or not, and for each folder the strongest status of
//! what changed under it. Worked out with each read, off the main thread.

use std::collections::HashMap;

use colosso_git::{Change, Status};
use gpui_kit::Hsla;

use crate::theme::Palette;

/// How a path changed, weakest to strongest: a folder shows the strongest under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mark {
    Untracked,
    Added,
    Deleted,
    Renamed,
    Modified,
    Conflicted,
}

impl Mark {
    pub fn of(status: &Status) -> Mark {
        match status {
            Status::Added => Mark::Added,
            Status::Untracked => Mark::Untracked,
            Status::Deleted => Mark::Deleted,
            Status::Modified => Mark::Modified,
            Status::Renamed { .. } => Mark::Renamed,
            Status::Conflicted => Mark::Conflicted,
        }
    }

    /// The letter the panels show for it.
    pub fn letter(self) -> &'static str {
        match self {
            Mark::Untracked => "U",
            Mark::Added => "A",
            Mark::Deleted => "D",
            Mark::Renamed => "R",
            Mark::Modified => "M",
            Mark::Conflicted => "!",
        }
    }

    /// The colour the panels show it in.
    pub fn colour(self, p: Palette) -> Hsla {
        match self {
            Mark::Added | Mark::Untracked => p.added,
            Mark::Deleted => p.removed,
            Mark::Modified => p.modified,
            Mark::Renamed => p.accent,
            Mark::Conflicted => p.danger,
        }
    }

    /// One file's two sides as one: a file new to the last commit, or moved since it, and
    /// edited after is still new, or moved.
    fn with(self, other: Mark) -> Mark {
        match (self, other) {
            (Mark::Added | Mark::Renamed, Mark::Modified) => self,
            (Mark::Modified, Mark::Added | Mark::Renamed) => other,
            _ => self.max(other),
        }
    }
}

/// A changed file as the tree marks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMark {
    pub mark: Mark,
    /// Whether some of the change isn't staged.
    pub unstaged: bool,
}

/// Every changed file's mark, and every folder's above them.
#[derive(Debug, Default)]
pub struct Marks {
    files: HashMap<String, FileMark>,
    folders: HashMap<String, Mark>,
}

impl Marks {
    pub fn new(listed: &[Change]) -> Marks {
        let mut marks = Marks::default();
        for change in listed {
            let mark = Mark::of(&change.status);
            let unstaged = !change.staged;
            if let Some(file) = marks.files.get_mut(&change.path) {
                file.mark = file.mark.with(mark);
                file.unstaged |= unstaged;
            } else {
                let file = FileMark { mark, unstaged };
                marks.files.insert(change.path.clone(), file);
            }
            let mut path = change.path.as_str();
            while let Some((folder, _)) = path.rsplit_once('/') {
                match marks.folders.get_mut(folder) {
                    Some(strongest) => *strongest = (*strongest).max(mark),
                    None => {
                        marks.folders.insert(folder.to_owned(), mark);
                    }
                }
                path = folder;
            }
        }
        marks
    }

    /// The mark of the file at `path`, relative to the checkout, if it changed.
    pub fn file(&self, path: &str) -> Option<FileMark> {
        self.files.get(path).copied()
    }

    /// The strongest mark under the folder at `path`, if anything under it changed.
    pub fn folder(&self, path: &str) -> Option<Mark> {
        self.folders.get(path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, status: Status, staged: bool) -> Change {
        Change {
            path: path.into(),
            status,
            staged,
        }
    }

    #[test]
    fn files_are_marked_by_both_sides_and_folders_by_the_strongest_under_them() {
        let marks = Marks::new(&[
            change("src/new.rs", Status::Added, true),
            change("src/new.rs", Status::Modified, false),
            change("src/old.rs", Status::Modified, true),
            change("src/deep/gone.rs", Status::Deleted, false),
            change("docs/draft.md", Status::Untracked, false),
            change("top.rs", Status::Conflicted, false),
        ]);
        let new = marks.file("src/new.rs").unwrap();
        assert_eq!((new.mark, new.unstaged), (Mark::Added, true));
        let old = marks.file("src/old.rs").unwrap();
        assert_eq!((old.mark, old.unstaged), (Mark::Modified, false));
        assert_eq!(marks.folder("src"), Some(Mark::Modified));
        assert_eq!(marks.folder("src/deep"), Some(Mark::Deleted));
        assert_eq!(marks.folder("docs"), Some(Mark::Untracked));
        assert_eq!(marks.folder(""), None);
        assert_eq!(
            marks.file("top.rs").map(|file| file.mark),
            Some(Mark::Conflicted)
        );
        assert_eq!(marks.file("src/same.rs"), None);
    }
}
