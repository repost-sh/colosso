//! Text as the protocol sees it: positions as lines and columns in an agreed encoding, and
//! open documents kept in step with the server edit by edit.

mod document;
mod line_index;

use lsp_types::PositionEncodingKind;

pub use document::{Document, Opened, Replaced};
pub use line_index::{EditError, IndexedText, LineIndex};

/// What a position's column counts within its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Encoding {
    /// Bytes of UTF-8.
    Utf8,
    /// Code units of UTF-16: two for a character outside the Basic Multilingual Plane, as an
    /// emoji is. The protocol's default, which every server speaks.
    #[default]
    Utf16,
}

impl Encoding {
    /// The encoding a server chose from those the client offered. One it didn't say, or one
    /// we never offered, is the protocol's default.
    pub fn negotiated(chosen: Option<&PositionEncodingKind>) -> Encoding {
        match chosen {
            Some(kind) if *kind == PositionEncodingKind::UTF8 => Encoding::Utf8,
            _ => Encoding::Utf16,
        }
    }

    /// How many of this encoding's units `text` takes. Most lines of code are ASCII, where
    /// UTF-16 units are bytes, and checking for that is quicker than counting.
    pub(crate) fn units(self, text: &str) -> usize {
        match self {
            Encoding::Utf16 if !text.is_ascii() => text.chars().map(char::len_utf16).sum(),
            Encoding::Utf8 | Encoding::Utf16 => text.len(),
        }
    }
}
