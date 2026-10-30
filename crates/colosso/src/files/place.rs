//! A place in a file to show: what the Files panel is asked to show by the tree, by Go to
//! File, and by a language server's definitions and symbols, each turned into this where
//! it's made.

use std::ops::Range;
use std::path::{Path, PathBuf};

use colosso_lsp::{Encoding, lsp_types};
use gpui_kit::SharedString;

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
    /// A range as a language server names it, in its encoding: made bytes of the text once
    /// that's shown, which is the text the server has when the file's open on it.
    Server(lsp_types::Range, Encoding),
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

/// `path` as the tree names it: relative to the checkout at `root`, or whole outside it.
pub fn relative(root: &Path, path: &Path) -> SharedString {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.display().to_string().into()
}
