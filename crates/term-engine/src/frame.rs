//! What a terminal looks like at one moment, owned: copied out of the engine on its thread
//! and handed to whoever paints or reads it, so no borrowed engine state crosses a thread or
//! outlives a frame. Rows are shared, so a frame whose rows didn't change costs a few clones.

use std::sync::Arc;

/// A colour, as the engine resolved it: palette indices are already looked up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// What one cell shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Grapheme {
    /// Nothing: a blank, or the tail of a wide character.
    #[default]
    Empty,
    /// One code point, the common case, kept inline.
    Char(char),
    /// A cluster of several code points shown as one glyph: a flag, a skin tone, a ZWJ
    /// sequence, a keycap.
    Cluster(Box<str>),
}

impl Grapheme {
    /// The text shown, empty for a blank.
    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Grapheme::Empty => "".into(),
            Grapheme::Char(c) => c.to_string().into(),
            Grapheme::Cluster(s) => s.as_ref().into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Grapheme::Empty)
    }
}

/// How wide a cell is and whether it's the second half of a wide character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Width {
    #[default]
    Narrow,
    /// Two cells wide; the next cell is its `SpacerTail`.
    Wide,
    /// The second half of a wide character: nothing to draw.
    SpacerTail,
    /// A blank at the end of a soft-wrapped line that a wide character didn't fit in.
    SpacerHead,
}

/// Underline styles, SGR 4 and its colon forms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// The boolean attributes of a cell, packed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attrs(u16);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1);
    pub const ITALIC: Attrs = Attrs(1 << 1);
    pub const FAINT: Attrs = Attrs(1 << 2);
    pub const BLINK: Attrs = Attrs(1 << 3);
    pub const INVERSE: Attrs = Attrs(1 << 4);
    pub const INVISIBLE: Attrs = Attrs(1 << 5);
    pub const STRIKETHROUGH: Attrs = Attrs(1 << 6);
    pub const OVERLINE: Attrs = Attrs(1 << 7);
    /// Part of the selection.
    pub const SELECTED: Attrs = Attrs(1 << 8);

    pub const fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }

    #[must_use]
    pub const fn with(self, other: Attrs) -> Attrs {
        Attrs(self.0 | other.0)
    }

    pub fn set(&mut self, other: Attrs, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// One cell of the grid.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub grapheme: Grapheme,
    /// `None` means the frame's default foreground.
    pub fg: Option<Rgb>,
    /// `None` means the frame's default background.
    pub bg: Option<Rgb>,
    /// `None` means the foreground.
    pub underline_color: Option<Rgb>,
    pub underline: Underline,
    pub width: Width,
    pub attrs: Attrs,
}

/// What a row is to the shell, from the OSC 133 marks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RowKind {
    #[default]
    Output,
    /// A primary prompt line.
    Prompt,
    /// A prompt continuation line.
    PromptContinuation,
}

/// One row of the viewport.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Soft-wrapped onto the next row.
    pub wrapped: bool,
    pub kind: RowKind,
}

/// The cursor's shape, DECSCUSR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
    /// A block outline, what an unfocused terminal shows.
    HollowBlock,
}

/// What's true of the cursor, packed like [`Attrs`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CursorFlags(u8);

impl CursorFlags {
    pub const NONE: CursorFlags = CursorFlags(0);
    /// Shown at all (DECTCEM).
    pub const VISIBLE: CursorFlags = CursorFlags(1);
    pub const BLINKING: CursorFlags = CursorFlags(1 << 1);
    /// On the tail of a wide character.
    pub const AT_WIDE_TAIL: CursorFlags = CursorFlags(1 << 2);
    /// At a password prompt, where typing should show nothing.
    pub const PASSWORD_INPUT: CursorFlags = CursorFlags(1 << 3);

    pub const fn contains(self, other: CursorFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn set(&mut self, other: CursorFlags, on: bool) {
        if on {
            self.0 |= other.0;
        } else {
            self.0 &= !other.0;
        }
    }
}

/// The cursor, when it's inside the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub flags: CursorFlags,
}

impl Cursor {
    pub fn visible(self) -> bool {
        self.flags.contains(CursorFlags::VISIBLE)
    }

    pub fn blinking(self) -> bool {
        self.flags.contains(CursorFlags::BLINKING)
    }
}

/// Where the viewport is in the scrollable area, in rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

impl Scrollbar {
    /// Whether the viewport shows the bottom, where new output lands.
    pub fn at_bottom(self) -> bool {
        self.offset.saturating_add(self.len) >= self.total
    }
}

/// How much of the frame changed since the last one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dirty {
    #[default]
    Clean,
    /// Only the rows flagged in [`Frame::dirty_rows`].
    Partial,
    Full,
}

/// One frame of a terminal.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Counts up with every frame the engine produced, for logs and screenshots.
    pub sequence: u64,
    pub cols: u16,
    pub rows: Vec<Arc<Row>>,
    /// Which rows changed since the previous frame, when `dirty` is `Partial`.
    pub dirty_rows: Vec<bool>,
    pub dirty: Dirty,
    pub cursor: Option<Cursor>,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor_color: Option<Rgb>,
    pub scrollbar: Scrollbar,
    pub alt_screen: bool,
    pub mouse_tracking: bool,
    pub title: String,
    /// The working directory the shell reported, a `file://` URI or empty.
    pub pwd: String,
}

impl Frame {
    pub fn row_count(&self) -> u16 {
        self.rows.len() as u16
    }

    /// The text of the viewport, one line per row, trailing blanks and blank rows trimmed.
    pub fn text(&self) -> String {
        let lines: Vec<String> = self
            .rows
            .iter()
            .map(|row| {
                let line: String = row
                    .cells
                    .iter()
                    .filter(|cell| cell.width != Width::SpacerTail)
                    .map(|cell| match &cell.grapheme {
                        Grapheme::Empty => std::borrow::Cow::Borrowed(" "),
                        other => other.text(),
                    })
                    .collect();
                line.trim_end().to_owned()
            })
            .collect();
        let kept = lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(0, |last| last + 1);
        lines[..kept].join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs_pack_and_unpack() {
        let mut attrs = Attrs::BOLD.with(Attrs::ITALIC);
        assert!(attrs.contains(Attrs::BOLD));
        assert!(attrs.contains(Attrs::ITALIC));
        assert!(!attrs.contains(Attrs::FAINT));
        attrs.set(Attrs::BOLD, false);
        assert!(!attrs.contains(Attrs::BOLD));
        assert!(attrs.contains(Attrs::ITALIC));
        attrs.set(Attrs::SELECTED, true);
        assert!(attrs.contains(Attrs::SELECTED));
    }

    #[test]
    fn text_trims_and_skips_wide_tails() {
        let wide = Cell {
            grapheme: Grapheme::Char('字'),
            width: Width::Wide,
            ..Cell::default()
        };
        let tail = Cell {
            width: Width::SpacerTail,
            ..Cell::default()
        };
        let a = Cell {
            grapheme: Grapheme::Char('a'),
            ..Cell::default()
        };
        let frame = Frame {
            sequence: 1,
            cols: 4,
            rows: vec![
                Arc::new(Row {
                    cells: vec![a.clone(), wide, tail, Cell::default()],
                    ..Row::default()
                }),
                Arc::new(Row {
                    cells: vec![Cell::default(), a, Cell::default(), Cell::default()],
                    ..Row::default()
                }),
            ],
            dirty_rows: vec![true, true],
            dirty: Dirty::Full,
            cursor: None,
            foreground: Rgb::default(),
            background: Rgb::default(),
            cursor_color: None,
            scrollbar: Scrollbar::default(),
            alt_screen: false,
            mouse_tracking: false,
            title: String::new(),
            pwd: String::new(),
        };
        assert_eq!(frame.text(), "a字\n a");
    }

    #[test]
    fn scrollbar_knows_the_bottom() {
        assert!(
            Scrollbar {
                total: 100,
                offset: 76,
                len: 24
            }
            .at_bottom()
        );
        assert!(
            !Scrollbar {
                total: 100,
                offset: 50,
                len: 24
            }
            .at_bottom()
        );
    }
}
