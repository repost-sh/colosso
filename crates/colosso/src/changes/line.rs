//! A line of a diff, or a pair of them side by side, as one element that paints itself: its
//! stretch of the file's card, the line numbers, the tint of an added or removed line and the
//! shaped code. Built from divs a line is half a dozen layout nodes and three text elements,
//! and jumping to a file builds a screenful of them at once.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::{
    App, AvailableSpace, BorderStyle, Bounds, ContentMask, Edges, Element, ElementId, Font,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, ShapedLine,
    SharedString, Style, TextAlign, TextRun, Window, WrappedLine, fill, point, px, quad, relative,
    size,
};

use super::highlight::Token;

/// What a line looks like, the same for every line of the diffs.
#[derive(Clone)]
pub struct Look {
    pub font: Font,
    pub font_size: Pixels,
    pub line_height: Pixels,
    /// How far in from the list's sides the file cards are.
    pub inset: Pixels,
    pub card: Hsla,
    pub hairline: Hsla,
    pub numbers: Hsla,
    /// Where a side by side pair has no line on one side.
    pub missing: Hsla,
    pub wrap: bool,
}

/// One side of a row: the line on it, or nothing where the other side has more lines.
pub struct Half {
    /// Its numbers from one, in unified rows the old and the new; `None` leaves a blank.
    pub numbers: Vec<Option<u32>>,
    pub number_width: Pixels,
    pub text: SharedString,
    /// The code's colours, covering the text.
    pub runs: Vec<TextRun>,
    /// The tint of an added or removed line.
    pub tint: Option<Hsla>,
    pub missing: bool,
}

impl Half {
    /// The side of a pair with no line on it.
    pub fn missing(number_width: Pixels) -> Half {
        Half {
            numbers: vec![None],
            number_width,
            text: SharedString::default(),
            runs: Vec::new(),
            tint: None,
            missing: true,
        }
    }
}

pub struct CodeLine {
    halves: Rc<[Half]>,
    look: Rc<Look>,
}

impl CodeLine {
    pub fn new(halves: Vec<Half>, look: Rc<Look>) -> Self {
        CodeLine {
            halves: halves.into(),
            look,
        }
    }
}

/// Where a half's parts go across a row `width` wide.
struct Columns {
    /// Each half's left edge and width.
    halves: Vec<(Pixels, Pixels)>,
}

/// Room left of the code, and right of it.
const CODE_LEFT: f32 = 8.;
const CODE_RIGHT: f32 = 12.;
/// Room right of a line number.
const NUMBER_RIGHT: f32 = 6.;

impl Columns {
    fn new(width: Pixels, halves: usize, look: &Look) -> Self {
        // Inside the card's two borders, a divider between the halves.
        let inner = width - look.inset * 2. - px(2.);
        let dividers = px((halves - 1) as f32);
        let half = (inner - dividers) / halves as f32;
        let left = look.inset + px(1.);
        Columns {
            halves: (0..halves)
                .map(|n| (left + (half + px(1.)) * n as f32, half))
                .collect(),
        }
    }

    /// Where `half`'s code starts, from its own left edge, and how wide it may be.
    fn code(&self, index: usize, half: &Half) -> (Pixels, Pixels) {
        let gutters = half.number_width * half.numbers.len() as f32;
        let left = gutters + px(CODE_LEFT);
        (
            left,
            (self.halves[index].1 - left - px(CODE_RIGHT)).max(px(0.)),
        )
    }
}

/// `half`'s code shaped to `width`, wrapped if the look wraps.
fn shape(half: &Half, width: Pixels, look: &Look, window: &Window) -> Option<WrappedLine> {
    if half.text.is_empty() {
        return None;
    }
    let wrap = look.wrap.then_some(width);
    let lines = window
        .text_system()
        .shape_text(half.text.clone(), look.font_size, &half.runs, wrap, None)
        .ok()?;
    lines.into_iter().next()
}

impl IntoElement for CodeLine {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

pub struct Shaped {
    columns: Columns,
    code: Vec<Option<WrappedLine>>,
    numbers: Vec<Vec<Option<ShapedLine>>>,
}

impl Element for CodeLine {
    type RequestLayoutState = ();
    type PrepaintState = Shaped;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let (halves, look) = (self.halves.clone(), self.look.clone());
        let layout = window.request_measured_layout(style, move |known, available, window, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(width) => width,
                _ => px(0.),
            });
            if !look.wrap {
                return size(width, look.line_height);
            }
            let columns = Columns::new(width, halves.len(), &look);
            let lines = (halves.iter().enumerate())
                .filter_map(|(index, half)| {
                    let (_, code_width) = columns.code(index, half);
                    shape(half, code_width, &look, window)
                })
                .map(|line| line.wrap_boundaries().len() + 1)
                .max()
                .unwrap_or(1);
            size(width, look.line_height * lines as f32)
        });
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) -> Shaped {
        let look = &self.look;
        let columns = Columns::new(bounds.size.width, self.halves.len(), look);
        // Shaped again as laying out shaped it, which GPUI keeps for the frame.
        let code = (self.halves.iter().enumerate())
            .map(|(index, half)| shape(half, columns.code(index, half).1, look, window))
            .collect();
        let numbers = (self.halves.iter())
            .map(|half| {
                let colour = half.tint.map_or(look.numbers, |tint| tint.alpha(0.8));
                (half.numbers.iter())
                    .map(|number| {
                        let text = SharedString::from(((*number)? + 1).to_string());
                        let run = TextRun {
                            len: text.len(),
                            font: look.font.clone(),
                            color: colour,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line =
                            window
                                .text_system()
                                .shape_line(text, look.font_size, &[run], None);
                        Some(line)
                    })
                    .collect()
            })
            .collect();
        Shaped {
            columns,
            code,
            numbers,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        shaped: &mut Shaped,
        window: &mut Window,
        cx: &mut App,
    ) {
        let look = &self.look;
        let height = bounds.size.height;
        let card = Bounds::new(
            point(bounds.left() + look.inset, bounds.top()),
            size(bounds.size.width - look.inset * 2., height),
        );
        window.paint_quad(quad(
            card,
            px(0.),
            look.card,
            Edges {
                left: px(1.),
                right: px(1.),
                ..Edges::default()
            },
            look.hairline,
            BorderStyle::Solid,
        ));
        for (index, half) in self.halves.iter().enumerate() {
            let (left, width) = shaped.columns.halves[index];
            let area = Bounds::new(
                point(bounds.left() + left, bounds.top()),
                size(width, height),
            );
            if index > 0 {
                let divider = Bounds::new(
                    point(area.left() - px(1.), area.top()),
                    size(px(1.), height),
                );
                window.paint_quad(fill(divider, look.hairline));
            }
            if half.missing {
                window.paint_quad(fill(area, look.missing));
                continue;
            }
            if let Some(tint) = half.tint {
                window.paint_quad(fill(area, tint.alpha(0.1)));
                let gutters = size(half.number_width * half.numbers.len() as f32, height);
                window.paint_quad(fill(Bounds::new(area.origin, gutters), tint.alpha(0.08)));
            }
            for (column, number) in shaped.numbers[index].iter().enumerate() {
                let Some(number) = number else {
                    continue;
                };
                let right = area.left() + half.number_width * (column + 1) as f32;
                let origin = point(right - px(NUMBER_RIGHT) - number.width, area.top());
                number
                    .paint(origin, look.line_height, TextAlign::Left, None, window, cx)
                    .ok();
            }
            let (code_left, code_width) = shaped.columns.code(index, half);
            if let Some(code) = &shaped.code[index] {
                let origin = point(area.left() + code_left, area.top());
                let clip = Bounds::new(origin, size(code_width + px(CODE_RIGHT), height));
                window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                    code.paint_background(
                        origin,
                        look.line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
                    code.paint(origin, look.line_height, TextAlign::Left, None, window, cx)
                        .ok();
                });
            }
        }
    }
}

/// The text runs for a line `len` bytes long: coloured by its `tokens`, which are in order and
/// don't overlap, plain elsewhere, with the changed `words` marked behind.
pub fn runs(
    len: usize,
    tokens: &[(Range<u32>, Token)],
    words: &[Range<usize>],
    colour: impl Fn(Option<Token>) -> Hsla,
    marked: Hsla,
    font: &Font,
) -> Vec<TextRun> {
    let mut edges: Vec<usize> = vec![0, len];
    for (range, _) in tokens {
        edges.extend([range.start as usize, range.end as usize]);
    }
    for range in words {
        edges.extend([range.start, range.end]);
    }
    edges.retain(|&edge| edge <= len);
    edges.sort_unstable();
    edges.dedup();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut token = tokens.iter().peekable();
    for pair in edges.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        while token
            .next_if(|(range, _)| range.end as usize <= start)
            .is_some()
        {}
        let kind = token
            .peek()
            .filter(|(range, _)| range.start as usize <= start)
            .map(|(_, kind)| *kind);
        let background = (words.iter())
            .any(|word| word.start <= start && end <= word.end)
            .then_some(marked);
        let color = colour(kind);
        match runs.last_mut() {
            Some(last) if last.color == color && last.background_color == background => {
                last.len += end - start
            }
            _ => runs.push(TextRun {
                len: end - start,
                font: font.clone(),
                color,
                background_color: background,
                underline: None,
                strikethrough: None,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use gpui_kit::{font, hsla};

    use super::*;

    fn runs_of(
        len: usize,
        tokens: &[(Range<u32>, Token)],
        words: &[Range<usize>],
    ) -> Vec<(usize, bool, bool)> {
        let plain = hsla(0., 0., 1., 1.);
        let marked = hsla(0.3, 1., 0.5, 0.3);
        runs(
            len,
            tokens,
            words,
            |kind| if kind.is_some() { marked } else { plain },
            marked,
            &font("Menlo"),
        )
        .into_iter()
        .map(|run| (run.len, run.color != plain, run.background_color.is_some()))
        .collect()
    }

    #[test]
    #[expect(clippy::single_range_in_vec_init, reason = "one changed word's bytes")]
    fn runs_cover_the_line_in_colours_and_marks() {
        // `fn total(x)`: a keyword, then plain with a changed word inside it.
        let runs = runs_of(11, &[(0..2, Token::Keyword)], &[(3..8)]);
        assert_eq!(
            runs,
            [
                (2, true, false),
                (1, false, false),
                (5, false, true),
                (3, false, false)
            ]
        );
        assert_eq!(runs.iter().map(|run| run.0).sum::<usize>(), 11);
    }

    #[test]
    fn a_plain_line_is_one_run_and_an_empty_one_none() {
        assert_eq!(runs_of(5, &[], &[]), [(5, false, false)]);
        assert!(runs_of(0, &[], &[]).is_empty());
    }
}
