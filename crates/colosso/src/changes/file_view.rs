//! A changed file as the changes panel shows it: its diff and, once worked out, its colours.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::OnceLock;

use colosso_git::{Change, Content, FileDiff, changed_words};

use super::highlight::{Highlights, highlight};

/// A changed file as shown: its diff and, once worked out, its colours. The diff shows plain
/// until then, as colouring takes longer than reading.
#[derive(Debug)]
pub struct FileView {
    pub diff: FileDiff,
    colours: OnceLock<Colours>,
}

/// What colouring a file works out ahead of drawing it, so drawing a line only looks it up.
#[derive(Debug, Default)]
pub struct Colours {
    /// Its syntax, if its kind of file is known.
    pub highlights: Option<Highlights>,
    /// The words that changed in each replaced line, by side and line.
    pub old_words: HashMap<u32, Vec<Range<usize>>>,
    pub new_words: HashMap<u32, Vec<Range<usize>>>,
}

impl FileView {
    pub fn new(diff: FileDiff) -> Self {
        FileView {
            diff,
            colours: OnceLock::new(),
        }
    }

    pub fn key(&self) -> Key {
        key(&self.diff.change)
    }

    pub fn colours(&self) -> Option<&Colours> {
        self.colours.get()
    }

    /// Works out its colours, unless they are already.
    pub fn colour(&self) {
        self.colours.get_or_init(|| colours(&self.diff));
    }
}

/// `diff`'s syntax, and the words each removed line and the line put in its place changed:
/// the nth of a hunk's removed lines is paired with the nth of its added ones.
fn colours(diff: &FileDiff) -> Colours {
    let mut colours = Colours {
        highlights: highlight(&diff.change.path, &diff.content),
        ..Colours::default()
    };
    if let Content::Text { old, new, hunks } = &diff.content {
        for hunk in hunks {
            for (old_line, new_line) in hunk.old.clone().zip(hunk.new.clone()) {
                let words = changed_words(old.line(old_line as usize), new.line(new_line as usize));
                if let Some((removed, added)) = words {
                    colours.old_words.insert(old_line, removed);
                    colours.new_words.insert(new_line, added);
                }
            }
        }
    }
    colours
}

/// What tells a file's changes apart: whether it's staged, and its path. A file partly staged
/// has a change on each side.
pub type Key = (bool, String);

pub fn key(change: &Change) -> Key {
    (change.staged, change.path.clone())
}
