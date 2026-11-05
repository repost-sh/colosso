//! How each layer of a terminal's frame is drawn, for the element: backgrounds in runs, the
//! selection, a cell's glyphs at its cell's origin (box drawing as shapes), underlines and
//! strikes, and an input method's composition.

use std::cell::RefCell;
use std::ops::Range;

use koyo_term_engine::{Attrs, Cell, Frame, Grapheme, Rgb, Underline, Width};
use gpui_kit::{
    App, Bounds, Hsla, Pixels, Rgba, SharedString, TextRun, Window, fill, point, px, size,
};

use crate::element::Grid;
use crate::glyphs::{Glyphs, Look, Style as GlyphStyle};
use crate::sprite;

/// One frame's painting, borrowed for the call.
pub(crate) struct Painter<'a> {
    pub frame: &'a Frame,
    pub grid: Grid,
    pub look: &'a Look,
    pub glyphs: &'a RefCell<Glyphs>,
}

impl Painter<'_> {
    fn row_y(&self, row: usize) -> Pixels {
        self.grid.origin.y + self.grid.metrics.cell_height * row as f32
    }

    fn span(&self, row: usize, start: usize, end: usize) -> Bounds<Pixels> {
        let cw = self.grid.metrics.cell_width;
        Bounds::new(
            point(self.grid.origin.x + cw * start as f32, self.row_y(row)),
            size(cw * (end - start) as f32, self.grid.metrics.cell_height),
        )
    }

    /// Runs of cells with the same background, then the selection over them.
    pub fn backgrounds(&self, window: &mut Window) {
        for (r, row) in self.frame.rows.iter().enumerate() {
            let cells = &row.cells;
            let mut c = 0;
            while c < cells.len() {
                let bg = colors(&cells[c], self.frame).1;
                let start = c;
                c += 1;
                while c < cells.len() && colors(&cells[c], self.frame).1 == bg {
                    c += 1;
                }
                if let Some(bg) = bg {
                    window.paint_quad(fill(self.span(r, start, c), hsla(bg)));
                }
            }
            let mut c = 0;
            while c < cells.len() {
                if !cells[c].attrs.contains(Attrs::SELECTED) {
                    c += 1;
                    continue;
                }
                let start = c;
                while c < cells.len() && cells[c].attrs.contains(Attrs::SELECTED) {
                    c += 1;
                }
                window.paint_quad(fill(self.span(r, start, c), self.look.selection));
            }
        }
    }

    pub fn text(&self, window: &mut Window) {
        let metrics = self.grid.metrics;
        for (r, row) in self.frame.rows.iter().enumerate() {
            let y = self.row_y(r);
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.width == Width::SpacerTail {
                    continue;
                }
                let (fg, _) = colors(cell, self.frame);
                let mut color = hsla(fg);
                if cell.attrs.contains(Attrs::FAINT) {
                    color = color.opacity(0.5);
                }
                let wide = if cell.width == Width::Wide { 2. } else { 1. };
                let x = self.grid.origin.x + metrics.cell_width * c as f32;
                let area = Bounds::new(
                    point(x, y),
                    size(metrics.cell_width * wide, metrics.cell_height),
                );
                if !cell.attrs.contains(Attrs::INVISIBLE) {
                    self.grapheme(cell, area, color, window);
                }
                self.decorations(cell, area, color, window);
            }
        }
    }

    /// Paints the cell's grapheme in `area`, in `color`.
    pub fn grapheme(&self, cell: &Cell, area: Bounds<Pixels>, color: Hsla, window: &mut Window) {
        let style = GlyphStyle {
            bold: cell.attrs.contains(Attrs::BOLD),
            italic: cell.attrs.contains(Attrs::ITALIC),
        };
        let pieces = match &cell.grapheme {
            Grapheme::Empty | Grapheme::Char(' ') => return,
            Grapheme::Char(c) => {
                if sprite::paint(*c, area, color, window) {
                    return;
                }
                self.glyphs.borrow_mut().char(*c, style, self.look, window)
            }
            Grapheme::Cluster(text) => self
                .glyphs
                .borrow_mut()
                .cluster(text, style, self.look, window),
        };
        let baseline = area.origin.y + self.grid.metrics.baseline;
        for piece in pieces.iter() {
            let origin = point(area.origin.x + piece.x, baseline);
            let painted = if piece.emoji {
                window.paint_emoji(origin, piece.font_id, piece.glyph, self.look.font_size)
            } else {
                window.paint_glyph(
                    origin,
                    piece.font_id,
                    piece.glyph,
                    self.look.font_size,
                    color,
                )
            };
            if let Err(error) = painted {
                log::debug!("couldn't paint a glyph: {error}");
            }
        }
    }

    fn decorations(&self, cell: &Cell, area: Bounds<Pixels>, color: Hsla, window: &mut Window) {
        let metrics = self.grid.metrics;
        let stroke = metrics.stroke;
        let line = |y: Pixels, color: Hsla, window: &mut Window| {
            window.paint_quad(fill(
                Bounds::new(point(area.origin.x, y), size(area.size.width, stroke)),
                color,
            ));
        };
        if cell.underline != Underline::None {
            let under = cell.underline_color.map_or(color, hsla);
            let y = area.origin.y + metrics.underline;
            line(y, under, window);
            if cell.underline == Underline::Double {
                line(y - stroke * 2., under, window);
            }
        }
        if cell.attrs.contains(Attrs::STRIKETHROUGH) {
            line(area.origin.y + metrics.baseline * 0.65, color, window);
        }
        if cell.attrs.contains(Attrs::OVERLINE) {
            line(area.origin.y, color, window);
        }
    }

    /// The underline under the link the pointer is over with ⌘ held, `cells` a run of
    /// columns per row: in each cell's own colour, which reads on any background.
    pub fn link(&self, cells: &[(u16, Range<u16>)], window: &mut Window) {
        let metrics = self.grid.metrics;
        let thickness = metrics.stroke.max(px(1.));
        for (row, cols) in cells {
            let Some(shown) = self.frame.rows.get(usize::from(*row)) else {
                continue;
            };
            let y = self.row_y(usize::from(*row)) + metrics.underline;
            for col in cols.clone() {
                let col = usize::from(col);
                // A wide character's tail is drawn in the colour of its head.
                let cell = match shown.cells.get(col) {
                    Some(cell) if cell.width == Width::SpacerTail && col > 0 => {
                        shown.cells.get(col - 1)
                    }
                    cell => cell,
                };
                let Some(cell) = cell else { continue };
                let x = self.grid.origin.x + metrics.cell_width * col as f32;
                window.paint_quad(fill(
                    Bounds::new(point(x, y), size(metrics.cell_width, thickness)),
                    hsla(colors(cell, self.frame).0),
                ));
            }
        }
    }

    /// An input method's composition at the cursor, underlined, over the cells it covers.
    pub fn composition(
        &self,
        text: &SharedString,
        area: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let run = TextRun {
            len: text.len(),
            font: self.look.font.clone(),
            color: hsla(self.frame.foreground),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(text.clone(), self.look.font_size, &[run], None);
        let width = line.width.max(area.size.width);
        let covered = Bounds::new(area.origin, size(width, area.size.height));
        window.paint_quad(fill(covered, hsla(self.frame.background)));
        let metrics = self.grid.metrics;
        // A line as tall as the cell centres its text as the cell's metrics do.
        if let Err(error) = line.paint(
            area.origin,
            metrics.cell_height,
            gpui_kit::TextAlign::Left,
            None,
            window,
            cx,
        ) {
            log::debug!("couldn't paint the composition: {error}");
        }
        window.paint_quad(fill(
            Bounds::new(
                point(area.origin.x, area.origin.y + metrics.underline),
                size(width, metrics.stroke),
            ),
            hsla(self.frame.foreground),
        ));
    }
}

/// A cell's foreground, and its background when it isn't the frame's.
fn colors(cell: &Cell, frame: &Frame) -> (Rgb, Option<Rgb>) {
    let fg = cell.fg.unwrap_or(frame.foreground);
    if cell.attrs.contains(Attrs::INVERSE) {
        return (cell.bg.unwrap_or(frame.background), Some(fg));
    }
    (fg, cell.bg)
}

pub fn hsla(rgb: Rgb) -> Hsla {
    Rgba {
        r: f32::from(rgb.r) / 255.,
        g: f32::from(rgb.g) / 255.,
        b: f32::from(rgb.b) / 255.,
        a: 1.,
    }
    .into()
}
