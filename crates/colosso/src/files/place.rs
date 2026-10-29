//! A place in a file to show: what the Files panel is asked to show by the tree, by Go to
//! File, and by a language server's definitions, each turned into this where it's made.

use std::ops::Range;
use std::path::PathBuf;

/// A file, relative to its checkout or absolute, and where in it the cursor goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub path: PathBuf,
    pub at: Option<At>,
}

/// Where in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum At {
    /// The start of a line, counted from 0.
    Line(u32),
    /// Bytes of the file's text as it's read, picked.
    Bytes(Range<usize>),
}

impl Place {
    /// The file at `path`, the cursor where it was or at its start.
    pub fn file(path: impl Into<PathBuf>) -> Place {
        Place {
            path: path.into(),
            at: None,
        }
    }
}
