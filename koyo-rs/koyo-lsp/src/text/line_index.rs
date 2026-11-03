//! Byte offsets in a text turned into the protocol's positions and back, and byte edits into
//! the protocol's edits.

use std::fmt;
use std::ops::Range;

use lsp_types::{Position, TextEdit};

use super::Encoding;

/// Where each line of a text starts, for turning byte offsets into positions and back in
/// either encoding. A conversion finds its line by binary search and walks only that line,
/// and skips even that on an ASCII line. Made in one pass over the text, and kept in step
/// with each edit by [`LineIndex::edit`].
///
/// It doesn't keep the text: each conversion is given the one it was made for. Given another,
/// its answers are wrong, though never out of the text.
///
/// Lines end at `\n`, and a `\r` before one ends the line with it: a position past a line's
/// end comes back as the offset before its `\r\n`, as the protocol says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// The byte each line starts at; the first is 0.
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        LineIndex {
            starts: std::iter::once(0).chain(line_starts(text, 0)).collect(),
        }
    }

    /// How many lines the text has: one more than its line breaks.
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// The position of byte `offset`. An offset inside a character stands for the
    /// character's start, and one past the end for the end.
    pub fn position(&self, text: &str, offset: usize, encoding: Encoding) -> Position {
        let offset = text.floor_char_boundary(offset);
        let line = self
            .starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let start = self.starts.get(line).copied().unwrap_or(0);
        let character = text
            .get(start..offset)
            .map_or(0, |before| encoding.units(before));
        Position::new(line as u32, character as u32)
    }

    /// The byte offset of `position`. Past the end of its line it's the line's end, past the
    /// last line the text's end, and inside a character (half a UTF-16 surrogate pair, or
    /// part of a UTF-8 sequence) the character's start.
    pub fn offset(&self, text: &str, position: Position, encoding: Encoding) -> usize {
        let line = position.line as usize;
        let Some(&start) = self.starts.get(line) else {
            return text.len();
        };
        let end = self
            .starts
            .get(line + 1)
            .map_or(text.len(), |&next| next - 1);
        let Some(line) = text.get(start..end) else {
            return text.floor_char_boundary(start);
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        let character = position.character as usize;
        start
            + match encoding {
                Encoding::Utf8 => line.floor_char_boundary(character),
                Encoding::Utf16 | Encoding::Utf32 if line.is_ascii() => character.min(line.len()),
                Encoding::Utf32 => (line.char_indices().nth(character)).map_or(line.len(), |(at, _)| at),
                Encoding::Utf16 => {
                    let mut units = 0;
                    line.char_indices()
                        .find(|&(_, char)| {
                            units += char.len_utf16();
                            units > character
                        })
                        .map_or(line.len(), |(at, _)| at)
                }
            }
    }

    /// The protocol's range for the bytes `range`.
    pub fn range(&self, text: &str, range: Range<usize>, encoding: Encoding) -> lsp_types::Range {
        lsp_types::Range::new(
            self.position(text, range.start, encoding),
            self.position(text, range.end, encoding),
        )
    }

    /// The bytes the protocol's `range` stands for, as [`Self::offset`] finds each end; a
    /// range ending before it starts is empty where it starts.
    pub fn offsets(&self, text: &str, range: lsp_types::Range, encoding: Encoding) -> Range<usize> {
        let start = self.offset(text, range.start, encoding);
        start..self.offset(text, range.end, encoding).max(start)
    }

    /// Keeps the index in step with the bytes `range` of its text replaced by `new_text`:
    /// the lines that started inside the range go, those `new_text` starts come, and those
    /// after move by the difference in length. Only the lines from the edit on are touched.
    pub fn edit(&mut self, range: Range<usize>, new_text: &str) {
        let range = range.start..range.end.max(range.start);
        // The lines starting inside the range: their `\n` was in it.
        let first = self.starts.partition_point(|&start| start <= range.start);
        let after = self.starts.partition_point(|&start| start <= range.end);
        for start in &mut self.starts[after..] {
            *start = *start - range.end + range.start + new_text.len();
        }
        self.starts
            .splice(first..after, line_starts(new_text, range.start));
    }
}

/// Where the lines after each `\n` in `text` start, `text` being at byte `at`.
fn line_starts(text: &str, at: usize) -> impl Iterator<Item = usize> {
    text.match_indices('\n')
        .map(move |(newline, _)| at + newline + 1)
}

/// An edit whose range isn't one of the text's: backwards, past its end, or splitting a
/// character.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError(pub Range<usize>);

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the edit at {:?} is outside the text", self.0)
    }
}

impl std::error::Error for EditError {}

/// A text and its line index, kept in step edit by edit: what turns byte edits into the
/// protocol's for a caller with no rope of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedText {
    text: String,
    lines: LineIndex,
}

impl IndexedText {
    pub fn new(text: String) -> IndexedText {
        let lines = LineIndex::new(&text);
        IndexedText { text, lines }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    /// As [`LineIndex::position`], in this text.
    pub fn position(&self, offset: usize, encoding: Encoding) -> Position {
        self.lines.position(&self.text, offset, encoding)
    }

    /// As [`LineIndex::offset`], in this text.
    pub fn offset(&self, position: Position, encoding: Encoding) -> usize {
        self.lines.offset(&self.text, position, encoding)
    }

    /// As [`LineIndex::offsets`], in this text.
    pub fn range(&self, range: Range<usize>, encoding: Encoding) -> lsp_types::Range {
        self.lines.range(&self.text, range, encoding)
    }

    pub fn offsets(&self, range: lsp_types::Range, encoding: Encoding) -> Range<usize> {
        self.lines.offsets(&self.text, range, encoding)
    }

    /// Replaces the bytes `range` with `new_text`, and returns the same edit as the protocol
    /// has it, its range in the text as it was. Edits made one after another and sent in
    /// that order, as [`Document::change`](super::Document::change) does, make the same text
    /// on the server.
    pub fn edit(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        encoding: Encoding,
    ) -> Result<TextEdit, EditError> {
        let inside = range.start <= range.end
            && self.text.is_char_boundary(range.start)
            && self.text.is_char_boundary(range.end);
        if !inside {
            return Err(EditError(range));
        }
        let edit = TextEdit::new(
            self.lines.range(&self.text, range.clone(), encoding),
            new_text.to_owned(),
        );
        self.text.replace_range(range.clone(), new_text);
        self.lines.edit(range, new_text);
        Ok(edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_count_the_negotiated_units() {
        // "a", an emoji (4 bytes, 2 UTF-16 units), "b".
        let text = "x\na😀b";
        let index = LineIndex::new(text);
        let b = "x\na😀".len();
        assert_eq!(index.position(text, b, Encoding::Utf8), Position::new(1, 5));
        assert_eq!(
            index.position(text, b, Encoding::Utf16),
            Position::new(1, 3)
        );
        assert_eq!(index.offset(text, Position::new(1, 5), Encoding::Utf8), b);
        assert_eq!(index.offset(text, Position::new(1, 3), Encoding::Utf16), b);
        assert_eq!(
            index.position(text, b, Encoding::Utf32),
            Position::new(1, 2)
        );
        assert_eq!(index.offset(text, Position::new(1, 2), Encoding::Utf32), b);
        assert_eq!(index.offset(text, Position::new(1, 9), Encoding::Utf32), text.len());
    }

    #[test]
    fn positions_outside_the_text_are_clamped() {
        let text = "ab\r\ncd";
        let index = LineIndex::new(text);
        // Past the end of a line: before its `\r\n`.
        assert_eq!(index.offset(text, Position::new(0, 9), Encoding::Utf16), 2);
        // Past the last line: the end.
        assert_eq!(index.offset(text, Position::new(5, 0), Encoding::Utf8), 6);
        assert_eq!(
            index.position(text, 99, Encoding::Utf8),
            Position::new(1, 2)
        );
    }

    #[test]
    fn positions_inside_a_character_mean_its_start() {
        let text = "😀中";
        let index = LineIndex::new(text);
        assert_eq!(index.offset(text, Position::new(0, 1), Encoding::Utf16), 0);
        assert_eq!(index.offset(text, Position::new(0, 2), Encoding::Utf8), 0);
        assert_eq!(index.offset(text, Position::new(0, 5), Encoding::Utf8), 4);
        assert_eq!(
            index.position(text, 2, Encoding::Utf16),
            Position::new(0, 0)
        );
    }

    #[test]
    fn ranges_come_back_as_the_bytes_they_stand_for() {
        let text = "x\na😀b\r\nc";
        let index = LineIndex::new(text);
        let emoji = "x\na".len().."x\na😀".len();
        let range = index.range(text, emoji.clone(), Encoding::Utf16);
        assert_eq!(index.offsets(text, range, Encoding::Utf16), emoji);
        // Backwards, it's empty where it starts; past the line's end, before its `\r\n`.
        let backwards = lsp_types::Range::new(Position::new(1, 3), Position::new(0, 0));
        assert_eq!(index.offsets(text, backwards, Encoding::Utf16), emoji.end..emoji.end);
        let past = lsp_types::Range::new(Position::new(1, 0), Position::new(1, 99));
        assert_eq!(index.offsets(text, past, Encoding::Utf8), 2.."x\na😀b".len());
    }

    #[test]
    fn edits_splice_the_line_starts() {
        let mut text = IndexedText::new("ab\ncd\nef\ngh".to_owned());
        // Joins three lines into one, then breaks one in two.
        text.edit(1..7, "X", Encoding::Utf8).unwrap();
        assert_eq!(text.as_str(), "aXf\ngh");
        text.edit(2..2, "\n\n", Encoding::Utf8).unwrap();
        assert_eq!(text.as_str(), "aX\n\nf\ngh");
        assert_eq!(text.lines(), &LineIndex::new(text.as_str()));
    }
}
