//! The cell grid's metrics and the glyphs painted into it. Each grapheme is shaped alone, once,
//! through GPUI's text system, and its glyphs kept by the grapheme and its style, so a frame
//! paints from the cache without shaping anything it painted before; every cluster sits in
//! its cell, ligatures across cells aren't formed (`docs/terminals.md` 7.2, step 3).

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{
    Font, FontId, FontStyle, FontWeight, GlyphId, Pixels, SharedString, TextRun, Window, px,
};

/// What a terminal looks like: the font and its size, the room between lines, the colours
/// the program doesn't set.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub font: Font,
    pub font_size: Pixels,
    /// Line height over the font's own ascent and descent.
    pub line_height: f32,
    pub padding: Pixels,
    pub foreground: koyo_term_engine::Rgb,
    pub background: koyo_term_engine::Rgb,
    pub cursor: gpui_kit::Hsla,
    pub selection: gpui_kit::Hsla,
}

/// The size of a cell and where text sits in it, snapped to device pixels so the grid
/// doesn't drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cell_width: Pixels,
    pub cell_height: Pixels,
    /// From the cell's top to the baseline.
    pub baseline: Pixels,
    pub underline: Pixels,
    pub stroke: Pixels,
}

/// A glyph of a shaped grapheme, offset from its cell's left edge.
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub font_id: FontId,
    pub glyph: GlyphId,
    pub x: Pixels,
    pub emoji: bool,
}

/// A grapheme's style as far as shaping goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
}

/// The glyphs of every grapheme painted so far, for one look at one scale.
#[derive(Default)]
pub struct Glyphs {
    built_for: Option<(Look, f32)>,
    metrics: Option<Metrics>,
    chars: HashMap<(char, Style), Rc<[Piece]>>,
    clusters: HashMap<(Box<str>, Style), Rc<[Piece]>>,
    /// Metrics at the font sizes a pinned grid was fitted with, by the size's bits, for the
    /// font, line height and scale they were measured in.
    sized_for: Option<(Font, u32, u32)>,
    sized: HashMap<u32, Metrics>,
}

impl Glyphs {
    /// The metrics for `look` in `window`, starting over when the look or the scale changed.
    pub fn prepare(&mut self, look: &Look, window: &Window) -> Metrics {
        let scale = window.scale_factor();
        if let (Some((built, at)), Some(metrics)) = (&self.built_for, self.metrics)
            && built == look
            && at.to_bits() == scale.to_bits()
        {
            return metrics;
        }
        self.chars.clear();
        self.clusters.clear();
        let metrics = measure(look, window);
        self.built_for = Some((look.clone(), scale));
        self.metrics = Some(metrics);
        metrics
    }

    /// The metrics `look` would have at `font_size`, without starting the glyphs over.
    pub fn metrics_at(&mut self, look: &Look, font_size: Pixels, window: &Window) -> Metrics {
        let key = (
            look.font.clone(),
            look.line_height.to_bits(),
            window.scale_factor().to_bits(),
        );
        if self.sized_for.as_ref() != Some(&key) {
            self.sized_for = Some(key);
            self.sized.clear();
        }
        let bits = f32::from(font_size).to_bits();
        if let Some(metrics) = self.sized.get(&bits) {
            return *metrics;
        }
        let sized = Look {
            font_size,
            ..look.clone()
        };
        let metrics = measure(&sized, window);
        self.sized.insert(bits, metrics);
        metrics
    }

    pub fn char(&mut self, c: char, style: Style, look: &Look, window: &Window) -> Rc<[Piece]> {
        if let Some(pieces) = self.chars.get(&(c, style)) {
            return Rc::clone(pieces);
        }
        let mut buf = [0u8; 4];
        let pieces = shape(c.encode_utf8(&mut buf), style, look, window);
        self.chars.insert((c, style), Rc::clone(&pieces));
        pieces
    }

    pub fn cluster(
        &mut self,
        text: &str,
        style: Style,
        look: &Look,
        window: &Window,
    ) -> Rc<[Piece]> {
        if let Some(pieces) = self.clusters.get(&(Box::from(text), style)) {
            return Rc::clone(pieces);
        }
        let pieces = shape(text, style, look, window);
        self.clusters
            .insert((Box::from(text), style), Rc::clone(&pieces));
        pieces
    }
}

fn measure(look: &Look, window: &Window) -> Metrics {
    let scale = window.scale_factor();
    let snap = |v: f32| (v * scale).round() / scale;
    let text = window.text_system();
    let font_id = text.resolve_font(&look.font);
    let size = look.font_size;
    let advance = text
        .advance(font_id, size, 'M')
        .map_or(f32::from(size) * 0.6, |advance| f32::from(advance.width));
    let ascent = f32::from(text.ascent(font_id, size));
    let descent = f32::from(text.descent(font_id, size)).abs();
    let cell_height = snap(((ascent + descent) * look.line_height).max(1.));
    let baseline = snap((cell_height - (ascent + descent)) / 2. + ascent);
    let stroke = snap((f32::from(size) / 14.).max(1. / scale));
    Metrics {
        cell_width: px(snap(advance).max(1. / scale)),
        cell_height: px(cell_height),
        baseline: px(baseline),
        underline: px(snap(baseline + descent / 2.).min(cell_height - stroke)),
        stroke: px(stroke),
    }
}

fn shape(text: &str, style: Style, look: &Look, window: &Window) -> Rc<[Piece]> {
    let mut font = look.font.clone();
    if style.bold {
        font.weight = FontWeight::BOLD;
    }
    if style.italic {
        font.style = FontStyle::Italic;
    }
    let run = TextRun {
        len: text.len(),
        font,
        color: gpui_kit::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(text.to_owned()),
        look.font_size,
        &[run],
        None,
    );
    line.runs
        .iter()
        .flat_map(|run| {
            run.glyphs.iter().map(|glyph| Piece {
                font_id: run.font_id,
                glyph: glyph.id,
                x: glyph.position.x,
                emoji: glyph.is_emoji,
            })
        })
        .collect()
}
