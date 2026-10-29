//! A visual row whose offsets remain in source UTF-8 bytes.
use gpui::{App, Pixels, Point, ShapedLine, SharedString, TextAlign, Window, point, px};
use std::ops::Range;

pub(crate) struct InlineFragment {
    pub(crate) range: Range<usize>,
    pub(crate) x: Pixels,
    pub(crate) width: Pixels,
    pub(crate) text: Option<ShapedLine>,
}

pub(crate) struct InputLine {
    pub(crate) len: usize,
    pub(crate) width: Pixels,
    /// The row's text from [`Self::text_offset`] on: all of it, except for
    /// a row shaped only where it can be seen, see [`Self::partly_shaped`].
    pub(crate) text: SharedString,
    content: Content,
}
// Keep the ordinary shaped row inline: boxing it would add an allocation to
// every existing plain-text row. Only token rows allocate fragment storage.
#[allow(clippy::large_enum_variant)]
enum Content {
    Text(ShapedLine),
    Inline(Vec<InlineFragment>),
    /// A row too long to shape whole: the part of it that can be seen is
    /// shaped; the bytes either side of it take `column_width` a character,
    /// which `columns` counts.
    Partial {
        shaped: ShapedLine,
        range: Range<usize>,
        column_width: Pixels,
        columns: PartialColumns,
    },
}

/// The characters of a partly shaped row before and after its shaped part.
pub(crate) struct PartialColumns {
    pub(crate) before: usize,
    pub(crate) after: usize,
}
impl From<ShapedLine> for InputLine {
    fn from(line: ShapedLine) -> Self {
        Self {
            len: line.len,
            width: line.width,
            text: line.text.clone(),
            content: Content::Text(line),
        }
    }
}
impl InputLine {
    pub(crate) fn inline(text: SharedString, fragments: Vec<InlineFragment>) -> Self {
        let width = fragments.last().map_or(px(0.), |f| f.x + f.width);
        Self {
            len: text.len(),
            text,
            width,
            content: Content::Inline(fragments),
        }
    }

    /// Where in the row [`Self::text`] starts.
    pub(crate) fn text_offset(&self) -> usize {
        match &self.content {
            Content::Partial { range, .. } => range.start,
            Content::Text(_) | Content::Inline(_) => 0,
        }
    }

    /// Where the shaped part of a partly shaped row starts.
    fn partial_x(column_width: Pixels, columns: &PartialColumns) -> Pixels {
        column_width * columns.before as f32
    }

    /// A row of `len` bytes of which only bytes `range` are shaped, as
    /// `shaped`: the characters before and after it are measured as
    /// `column_width` each, so the shaped part starts `columns.before` of
    /// them from the row's start.
    pub(crate) fn partly_shaped(
        len: usize,
        range: Range<usize>,
        shaped: ShapedLine,
        column_width: Pixels,
        columns: PartialColumns,
    ) -> Self {
        Self {
            len,
            width: Self::partial_x(column_width, &columns)
                + shaped.width
                + column_width * columns.after as f32,
            text: shaped.text.clone(),
            content: Content::Partial {
                shaped,
                range,
                column_width,
                columns,
            },
        }
    }
    pub(crate) fn x_for_index(&self, ix: usize) -> Pixels {
        match &self.content {
            Content::Text(line) => line.x_for_index(ix),
            // Outside the shaped part bytes are spread evenly over its
            // characters: exact for ASCII, close enough to scroll to.
            Content::Partial {
                shaped,
                range,
                column_width,
                columns,
            } => {
                let x = Self::partial_x(*column_width, columns);
                if ix < range.start {
                    x * ix as f32 / range.start as f32
                } else if ix <= range.end {
                    x + shaped.x_for_index(ix - range.start)
                } else {
                    let after = self.len - range.end;
                    x + shaped.width
                        + *column_width * columns.after as f32 * (ix - range.end).min(after) as f32
                            / after as f32
                }
            }
            Content::Inline(fragments) => {
                for f in fragments {
                    if ix < f.range.end {
                        return f.x
                            + f.text.as_ref().map_or(
                                if ix <= f.range.start { px(0.) } else { f.width },
                                |line| line.x_for_index(ix.saturating_sub(f.range.start)),
                            );
                    }
                }
                self.width
            }
        }
    }
    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        match &self.content {
            Content::Text(line) => line.closest_index_for_x(x),
            // Only the shaped part's indices are known to be character
            // boundaries: a point outside it goes to its nearer end.
            Content::Partial {
                shaped,
                range,
                column_width,
                columns,
            } => {
                let start = Self::partial_x(*column_width, columns);
                if x < start {
                    range.start
                } else {
                    range.start + shaped.closest_index_for_x(x - start)
                }
            }
            Content::Inline(fragments) => {
                for f in fragments {
                    if x <= f.x + f.width {
                        return f.text.as_ref().map_or(
                            if x - f.x < f.width / 2. {
                                f.range.start
                            } else {
                                f.range.end
                            },
                            |line| f.range.start + line.closest_index_for_x(x - f.x),
                        );
                    }
                }
                self.len
            }
        }
    }
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        match &self.content {
            Content::Text(line) => line.index_for_x(x),
            Content::Inline(_) | Content::Partial { .. } => {
                (x >= px(0.) && x <= self.width).then(|| self.closest_index_for_x(x))
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, false, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, true, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_pass(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        background: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let paint = |line: &ShapedLine, pos, align, width, window: &mut Window, cx: &mut App| {
            if background {
                let _ = line.paint_background(pos, height, align, width, window, cx);
            } else {
                let _ = line.paint(pos, height, align, width, window, cx);
            }
        };
        match &self.content {
            Content::Text(line) => paint(line, pos, align, width, window, cx),
            Content::Partial {
                shaped,
                column_width,
                columns,
                ..
            } => paint(
                shaped,
                pos + point(Self::partial_x(*column_width, columns), px(0.)),
                TextAlign::Left,
                None,
                window,
                cx,
            ),
            Content::Inline(fragments) => {
                let remaining = (width.unwrap_or(self.width) - self.width).max(px(0.));
                let offset = match align {
                    TextAlign::Right => remaining,
                    TextAlign::Center => remaining / 2.,
                    _ => px(0.),
                };
                for f in fragments {
                    if let Some(line) = &f.text {
                        paint(
                            line,
                            pos + point(offset + f.x, px(0.)),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                }
            }
        }
    }
}
