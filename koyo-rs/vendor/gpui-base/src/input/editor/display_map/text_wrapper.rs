use super::inline_line::InputLine;
use gpui::Half;
use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use gpui::{
    App, Font, LineFragment, Pixels, Point, ShapedLine, Size, TextAlign, Window, point, px, size,
};
use ropey::{LineType, Rope, RopeSlice};
use smallvec::SmallVec;
use sum_tree::{Bias, Dimensions, SumTree};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::async_util::cancelled;
use crate::input::{
    Point as TreeSitterPoint, RopeExt, TabSize,
    layout::{LastLayout, WhitespaceIndicators},
    tab_stops::{expanded_width, tab_stop_width},
};

/// Controls how soft-wrapped continuation lines are indented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WrappingIndent {
    /// Continuation lines start flush-left at the full editor width.
    None,
    /// Continuation lines keep the same indentation as the first line.
    #[default]
    Same,
}

/// Graphemes measured first for the first row of a line. Most lines are shorter
/// and end on their first row with one measurement; later rows start from the
/// length of the row before them.
const FIRST_ROW_PROBE: usize = 128;

/// Choose Unicode line-break opportunities using the same shaped widths as
/// painting. Oversized words fall back to complete graphemes, never UTF-8 bytes.
fn measured_wrap_boundaries(
    text: &str,
    width: Pixels,
    wrapping_indent: WrappingIndent,
    mut measure: impl FnMut(&str) -> Pixels,
) -> Vec<gpui::Boundary> {
    let indent = if wrapping_indent == WrappingIndent::Same {
        text.chars()
            .take_while(|&c| c == ' ')
            .count()
            .min(gpui::LineWrapper::MAX_INDENT as usize)
    } else {
        0
    };
    let indent_width = measure(&text[..indent]);
    let ends: Vec<usize> = text
        .grapheme_indices(true)
        .map(|(ix, grapheme)| ix + grapheme.len())
        .collect();
    let opportunities: Vec<usize> = unicode_linebreak::linebreaks(text)
        .map(|(ix, _)| ix)
        .filter(|ix| ends.binary_search(ix).is_ok())
        .collect();
    let mut result = Vec::new();
    let mut first = 0;
    let mut start = 0;
    let mut probe = FIRST_ROW_PROBE;
    while first < ends.len() {
        let available = if start == 0 {
            width
        } else {
            width - indent_width
        };
        // Find the fitting prefix locally. Exponential probing from a guess
        // avoids shaping the entire remaining logical line for every visual
        // row of a long paste, and settles a line that fits in one shaping.
        let remaining = ends.len() - first;
        let mut low = 0;
        let mut high = probe.clamp(1, remaining);
        while measure(&text[start..ends[first + high - 1]]) <= available {
            low = high;
            if high == remaining {
                break;
            }
            high = (high * 2).min(remaining);
        }
        while low + 1 < high {
            let mid = (low + high) / 2;
            if measure(&text[start..ends[first + mid - 1]]) <= available {
                low = mid;
            } else {
                high = mid;
            }
        }
        // An indivisible grapheme wider than the viewport still consumes a row.
        let fitting_end = ends[first + low.max(1) - 1];
        if fitting_end == text.len() {
            break;
        }
        let candidate = opportunities.partition_point(|&ix| ix <= fitting_end);
        let end = candidate
            .checked_sub(1)
            .map(|ix| opportunities[ix])
            .filter(|&ix| ix > start && (start != 0 || ix > indent))
            .unwrap_or(fitting_end);
        result.push(gpui::Boundary {
            ix: end,
            next_indent: indent as u32,
        });
        let next = ends.partition_point(|&ix| ix <= end);
        probe = next - first;
        first = next;
        start = end;
    }
    result
}

/// A line with soft wrapped lines info.
#[derive(Debug, Clone)]
pub(crate) struct LineItem {
    /// The byte length of the line, without the end `\n`.
    len: usize,
    /// Number of leading characters of the line reserved as indentation for continuation wrapped
    /// lines, when [`WrappingIndent::Same`] is used.
    ///
    /// Zero when [`WrappingIndent::None`] is used or the line is not wrapped.
    pub(crate) indent: u32,
    /// The soft wrapped lines relative byte range (0..len) of this line (Include first line).
    ///
    /// Not contains the line end `\n`.
    pub(crate) wrapped_lines: SmallVec<[Range<usize>; 1]>,
    /// The [`TextWrapper::generation`] of the parameters this line was wrapped with: while it
    /// is an earlier one, the line keeps an earlier wrapping until it is wrapped again.
    pub(crate) generation: u32,
}

impl LineItem {
    /// A line of `len` bytes on one row, as when nothing wraps.
    fn unwrapped(len: usize, generation: u32) -> Self {
        Self {
            len,
            indent: 0,
            wrapped_lines: smallvec::smallvec![0..len],
            generation,
        }
    }

    /// Get the bytes length of this line.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Get number of soft wrapped lines of this line (include the first line).
    #[inline]
    pub(crate) fn lines_len(&self) -> usize {
        self.wrapped_lines.len()
    }
}

/// Summary of a subtree of [`LineItem`]s, maintained incrementally by the [`SumTree`].
#[derive(Debug, Clone)]
pub(crate) struct LineSummary {
    /// Number of buffer lines.
    buffer_rows: usize,
    /// Number of wrap rows (sum of each line's `lines_len()`).
    wrap_rows: usize,
    /// Sum of byte lengths of the buffer lines (without the trailing `\n`).
    bytes: usize,
    /// Byte length of the longest line in this subtree.
    max_line_len: usize,
    /// Buffer row (relative to this subtree) of the first line achieving `max_line_len`.
    longest_row: usize,
}

impl sum_tree::Summary for LineSummary {
    type Context<'a> = &'a ();

    fn zero(_: &()) -> Self {
        LineSummary {
            buffer_rows: 0,
            wrap_rows: 0,
            bytes: 0,
            max_line_len: 0,
            longest_row: 0,
        }
    }

    fn add_summary(&mut self, other: &Self, _: &()) {
        // Keep the leftmost row that achieves a strictly greater length
        if other.max_line_len > self.max_line_len {
            self.longest_row = self.buffer_rows + other.longest_row;
            self.max_line_len = other.max_line_len;
        }
        self.buffer_rows += other.buffer_rows;
        self.wrap_rows += other.wrap_rows;
        self.bytes += other.bytes;
    }
}

impl sum_tree::Item for LineItem {
    type Summary = LineSummary;

    fn summary(&self, _: &()) -> LineSummary {
        LineSummary {
            buffer_rows: 1,
            wrap_rows: self.lines_len(),
            bytes: self.len(),
            max_line_len: self.len(),
            longest_row: 0,
        }
    }
}

/// Cursor dimension counting buffer rows.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BufferRows(pub usize);

impl<'a> sum_tree::Dimension<'a, LineSummary> for BufferRows {
    fn zero(_: &()) -> Self {
        BufferRows(0)
    }

    fn add_summary(&mut self, summary: &'a LineSummary, _: &()) {
        self.0 += summary.buffer_rows;
    }
}

/// Cursor dimension counting wrap rows.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WrapRows(pub usize);

impl<'a> sum_tree::Dimension<'a, LineSummary> for WrapRows {
    fn zero(_: &()) -> Self {
        WrapRows(0)
    }

    fn add_summary(&mut self, summary: &'a LineSummary, _: &()) {
        self.0 += summary.wrap_rows;
    }
}

/// Used to prepare the text with soft wrap to be get lines to displayed in the Editor.
///
/// After use lines to calculate the scroll size of the Editor.
pub(crate) struct TextWrapper {
    text: Rope,
    params: WrapParams,
    /// The lines by split \n
    pub(crate) lines: SumTree<LineItem>,

    inline_metrics: Arc<[(Range<usize>, Pixels)]>,
    _initialized: bool,
    /// Advances whenever what lines are wrapped by changes. A line wrapped with
    /// an earlier generation is stale: it keeps its earlier wrapping, as an
    /// interpolation, until it is wrapped again.
    generation: u32,
    /// How many lines are stale.
    stale_rows: usize,
    /// The generation a background wrap is under way for, and the edits made
    /// since it took its snapshot of the text, to apply to its result: the
    /// lines replaced, and the lines that replaced them.
    background: Option<(u32, Vec<(Range<usize>, Vec<LineItem>)>)>,
    /// Shapes the lines wrapped on this thread. It is kept from one update to
    /// the next, so lines measured before come from its cache.
    measurer: Option<LineMeasurer>,
}

/// What lines are wrapped by.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WrapParams {
    pub(crate) font: Font,
    pub(crate) font_size: Pixels,
    /// None when nothing wraps.
    pub(crate) wrap_width: Option<Pixels>,
    pub(crate) wrapping_indent: WrappingIndent,
    /// Spaces per tab stop, so tabs are measured as they are drawn.
    pub(crate) tab_size: usize,
}

impl WrapParams {
    /// What text is measured by: everything but the width it wraps at.
    fn measuring(&self) -> Self {
        Self {
            wrap_width: None,
            ..self.clone()
        }
    }
}

/// Bytes of text a [`LineMeasurer`] shapes before it starts over with an
/// empty cache, so the cache stays small however much text goes through it.
const MEASURER_CACHE_BYTES: usize = 512 * 1024;

/// Bytes of text replacing the whole text past which its lines are left to
/// wrap where they are seen and in the background, see
/// [`TextWrapper::reset_lines`].
const SYNC_WRAP_BYTES: usize = 64 * 1024;

/// Lines between checks for cancellation in a background wrap.
const BACKGROUND_CANCEL_CHECK: usize = 256;

/// Finds where lines wrap by shaping them as they are painted.
struct LineMeasurer {
    text_system: Arc<gpui::TextSystem>,
    shaper: gpui::WindowTextSystem,
    line_wrapper: gpui::LineWrapperHandle,
    /// What it measures by, see [`WrapParams::measuring`].
    params: WrapParams,
    tab_stop: Pixels,
    /// Bytes shaped since the cache started over.
    shaped: usize,
}

impl LineMeasurer {
    fn new(text_system: &Arc<gpui::TextSystem>, params: &WrapParams) -> Self {
        let shaper = gpui::WindowTextSystem::new(text_system.clone());
        let tab_stop = tab_stop_width(&shaper, &params.font, params.font_size, params.tab_size);
        Self {
            text_system: text_system.clone(),
            line_wrapper: text_system.line_wrapper(params.font.clone(), params.font_size),
            shaper,
            params: params.measuring(),
            tab_stop,
            shaped: 0,
        }
    }

    fn measures_for(&self, params: &WrapParams) -> bool {
        self.params == params.measuring()
    }

    /// The boundaries of the rows `line_str` wraps into at `wrap_width`;
    /// `line_start` is its offset in the text, which `metrics` are relative to.
    fn wrap(
        &mut self,
        line_str: &str,
        wrap_width: Pixels,
        line_start: usize,
        metrics: &[(Range<usize>, Pixels)],
    ) -> Vec<gpui::Boundary> {
        if self.shaped > MEASURER_CACHE_BYTES {
            self.shaper = gpui::WindowTextSystem::new(self.text_system.clone());
            self.shaped = 0;
        }
        let mut fragments = Vec::new();
        let mut offset = 0;
        let first = metrics.partition_point(|(r, _)| r.end <= line_start);
        for (range, width) in &metrics[first..] {
            if range.start >= line_start + line_str.len() {
                break;
            }
            if range.start < line_start || range.end > line_start + line_str.len() {
                continue;
            }
            let range = range.start - line_start..range.end - line_start;
            if !line_str.is_char_boundary(range.start) || !line_str.is_char_boundary(range.end) {
                continue;
            }
            if offset < range.start {
                fragments.push(LineFragment::text(&line_str[offset..range.start]));
            }
            fragments.push(LineFragment::element(*width, range.len()));
            offset = range.end;
        }
        if fragments.is_empty() {
            let Self {
                shaper,
                params,
                tab_stop,
                shaped,
                ..
            } = self;
            return measured_wrap_boundaries(line_str, wrap_width, params.wrapping_indent, |text| {
                *shaped += text.len();
                let layout = shaper.layout_line(
                    text,
                    params.font_size,
                    &[gpui::TextRun {
                        len: text.len(),
                        font: params.font.clone(),
                        color: gpui::black(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    }],
                    None,
                );
                expanded_width(&layout, text, *tab_stop)
            });
        }
        if offset < line_str.len() {
            fragments.push(LineFragment::text(&line_str[offset..]));
        }
        self.line_wrapper
            .wrap_line(&fragments, wrap_width, gpui::IndentAdjustment::SameIndent)
            .collect()
    }
}

/// The lines `rows` of `text`, without their `\n`, each with the offset it
/// starts at, in one pass over the text.
fn text_lines(text: &Rope, rows: Range<usize>) -> impl Iterator<Item = (RopeSlice<'_>, usize)> {
    let first = rows.start.min(text.lines_len());
    let mut start = text.line_start_offset(first);
    text.lines_at(first, LineType::LF)
        .take(rows.end.saturating_sub(first))
        .map(move |line| {
            let line_start = start;
            start += line.len();
            let ends_line = line.len() > 0 && line.byte(line.len() - 1) == b'\n';
            (line.slice(..line.len() - usize::from(ends_line)), line_start)
        })
}

/// `line`, which starts at `line_start` in the text, wrapped by `wrap_line`
/// as `params` say: not at all without a wrap width.
fn wrap_line_item<F>(
    line: RopeSlice<'_>,
    line_start: usize,
    params: &WrapParams,
    generation: u32,
    wrap_line: &mut F,
) -> LineItem
where
    F: FnMut(&str, Pixels, usize) -> Vec<gpui::Boundary>,
{
    let Some(wrap_width) = params.wrap_width else {
        return LineItem::unwrapped(line.len(), generation);
    };
    let mut wrapped_lines = SmallVec::<[Range<usize>; 1]>::new();
    let mut prev_boundary_ix = 0;
    let mut indent_chars = 0;
    // Borrowed for lines within a single rope chunk.
    let line_str: Cow<str> = line.into();
    match params.wrapping_indent {
        WrappingIndent::Same => {
            // Here only have wrapped line, if there is no wrap meet, the `line_wraps`
            // result will empty.
            for boundary in wrap_line(&line_str, wrap_width, line_start) {
                wrapped_lines.push(prev_boundary_ix..boundary.ix);
                prev_boundary_ix = boundary.ix;
                indent_chars = boundary.next_indent;
            }
        }
        WrappingIndent::None => {
            // The first visual line keeps the line's leading indentation, so it is
            // wrapped as is.
            let boundaries = wrap_line(&line_str, wrap_width, line_start);
            if let Some(first_ix) = boundaries.first().map(|b| b.ix) {
                wrapped_lines.push(prev_boundary_ix..first_ix);
                prev_boundary_ix = first_ix;

                for boundary in
                    wrap_line(&line_str[first_ix..], wrap_width, line_start + first_ix)
                {
                    let ix = first_ix + boundary.ix;
                    wrapped_lines.push(prev_boundary_ix..ix);
                    prev_boundary_ix = ix;
                }
            }
        }
    }

    // Reset of the line
    if prev_boundary_ix < line.len() || prev_boundary_ix == 0 {
        wrapped_lines.push(prev_boundary_ix..line.len());
    }

    LineItem {
        len: line.len(),
        indent: indent_chars,
        wrapped_lines,
        generation,
    }
}

/// Replace `rows` of `lines` with `new_lines`.
fn splice_rows(lines: &mut SumTree<LineItem>, rows: Range<usize>, new_lines: Vec<LineItem>) {
    if lines.is_empty() {
        *lines = SumTree::from_iter(new_lines, &());
        return;
    }
    let mut cursor = lines.cursor::<BufferRows>(&());
    let mut new_tree = cursor.slice(&BufferRows(rows.start), Bias::Right);
    // Skip the replaced rows
    cursor.seek_forward(&BufferRows(rows.end), Bias::Right);
    new_tree.extend(new_lines, &());
    // Untouched rows after the edit
    new_tree.append(cursor.suffix(), &());
    drop(cursor);
    *lines = new_tree;
}

/// A wrap of a whole text for a background thread, with everything it needs.
pub(crate) struct WrapJob {
    text: Rope,
    /// The lines of `text`: the stale ones are the ones to wrap.
    lines: SumTree<LineItem>,
    params: WrapParams,
    inline_metrics: Arc<[(Range<usize>, Pixels)]>,
    generation: u32,
    text_system: Arc<gpui::TextSystem>,
}

/// The lines a [`WrapJob`] wrapped.
pub(crate) struct WrappedLines {
    generation: u32,
    lines: SumTree<LineItem>,
}

/// `lines` of `text`, the ones stale for `generation` wrapped again by
/// `wrap_line`, or `None` once `cancel` is set.
fn wrap_all_lines<F>(
    text: &Rope,
    lines: &SumTree<LineItem>,
    params: &WrapParams,
    generation: u32,
    cancel: &AtomicBool,
    wrap_line: &mut F,
) -> Option<SumTree<LineItem>>
where
    F: FnMut(&str, Pixels, usize) -> Vec<gpui::Boundary>,
{
    let mut wrapped = Vec::with_capacity(lines.summary().buffer_rows);
    for (row, ((line, start), item)) in text_lines(text, 0..text.lines_len())
        .zip(lines.iter())
        .enumerate()
    {
        if row % BACKGROUND_CANCEL_CHECK == 0 && cancelled(cancel) {
            return None;
        }
        wrapped.push(if item.generation == generation {
            item.clone()
        } else {
            wrap_line_item(line, start, params, generation, wrap_line)
        });
    }
    Some(SumTree::from_iter(wrapped, &()))
}

impl WrapJob {
    /// Wraps the stale lines, or gives up once `cancel` is set.
    pub(crate) fn run(self, cancel: &AtomicBool) -> Option<WrappedLines> {
        let mut measurer = LineMeasurer::new(&self.text_system, &self.params);
        let lines = wrap_all_lines(
            &self.text,
            &self.lines,
            &self.params,
            self.generation,
            cancel,
            &mut |line, width, start| measurer.wrap(line, width, start, &self.inline_metrics),
        )?;
        Some(WrappedLines {
            generation: self.generation,
            lines,
        })
    }
}

#[allow(unused)]
impl TextWrapper {
    pub(crate) fn new(font: Font, font_size: Pixels, wrap_width: Option<Pixels>) -> Self {
        Self {
            text: Rope::new(),
            params: WrapParams {
                font,
                font_size,
                wrap_width,
                wrapping_indent: WrappingIndent::default(),
                tab_size: TabSize::default().tab_size,
            },
            lines: SumTree::new(&()),
            inline_metrics: Arc::from([]),
            _initialized: false,
            generation: 0,
            stale_rows: 0,
            background: None,
            measurer: None,
        }
    }

    #[inline]
    pub(crate) fn set_default_text(&mut self, text: &Rope) {
        self.text = text.clone();
        // A background wrap of the text it replaced doesn't apply to it.
        self.background = None;
    }

    /// Get reference to the rope text.
    #[inline]
    pub(crate) fn text(&self) -> &Rope {
        &self.text
    }

    /// Get the total number of lines including wrapped lines.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.lines.summary().wrap_rows
    }

    /// Get the total number of buffer lines.
    #[inline]
    pub(crate) fn lines_count(&self) -> usize {
        self.lines.summary().buffer_rows
    }

    /// Get the 0-based row index of the longest line (by byte length).
    #[inline]
    pub(crate) fn longest_row(&self) -> usize {
        self.lines.summary().longest_row
    }

    /// Get the line item by buffer row index.
    #[inline]
    pub(crate) fn line(&self, row: usize) -> Option<&LineItem> {
        let mut cursor = self.lines.cursor::<BufferRows>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        cursor.item()
    }

    /// Iterate buffer lines in order.
    #[inline]
    pub(crate) fn iter_lines(&self) -> impl Iterator<Item = &LineItem> {
        self.lines.iter()
    }

    /// First wrap row of buffer line `row`. Returns the total wrap row count if `row` is
    /// out of range.
    pub(crate) fn buffer_line_to_first_wrap_row(&self, row: usize) -> usize {
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        cursor.start().1.0
    }

    /// Wrap row range of buffer line `row`.
    pub(crate) fn buffer_line_to_wrap_row_range(&self, row: usize) -> Range<usize> {
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        let start = cursor.start().1.0;
        let len = cursor.item().map(|l| l.lines_len()).unwrap_or(0);
        start..start + len
    }

    /// Buffer line containing wrap row `wrap_row`, clamped to the last line.
    pub(crate) fn wrap_row_to_buffer_line(&self, wrap_row: usize) -> usize {
        let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(&());
        cursor.seek(&WrapRows(wrap_row), Bias::Right);
        match cursor.item() {
            Some(_) => cursor.start().1.0,
            None => self.lines_count().saturating_sub(1),
        }
    }

    /// What lines are wrapped by.
    pub(crate) fn params(&self) -> &WrapParams {
        &self.params
    }

    /// Changes what lines are wrapped by without wrapping any again: until
    /// they are, every line keeps its earlier wrapping, as an interpolation.
    /// Without a wrap width, though, every line is one row at once, which
    /// takes no shaping. Returns whether anything changed.
    pub(crate) fn set_params(&mut self, params: WrapParams) -> bool {
        if params == self.params {
            return false;
        }
        self.params = params;
        self.generation = self.generation.wrapping_add(1);
        // A background wrap with the parameters before doesn't apply any more.
        self.background = None;
        if self.params.wrap_width.is_none() {
            self.reset_lines();
        } else {
            self.stale_rows = self.lines_count();
        }
        true
    }

    /// Every line of the text as one row, in one pass over it: as they are
    /// without a wrap width, and otherwise stale, to wrap again where they
    /// are seen and in the background.
    fn reset_lines(&mut self) {
        let fresh = self.params.wrap_width.is_none();
        let generation = if fresh {
            self.generation
        } else {
            self.generation.wrapping_sub(1)
        };
        let lines = text_lines(&self.text, 0..self.text.lines_len())
            .map(|(line, _)| LineItem::unwrapped(line.len(), generation));
        self.lines = SumTree::from_iter(lines, &());
        self.stale_rows = if fresh { 0 } else { self.lines_count() };
        self.background = None;
    }

    /// How many lines keep an earlier wrapping until they are wrapped again.
    #[inline]
    pub(crate) fn stale_rows(&self) -> usize {
        self.stale_rows
    }

    /// The measurer for the current parameters, the one kept from before if
    /// it measures by them.
    fn take_measurer(&mut self, cx: &App) -> LineMeasurer {
        match self.measurer.take() {
            Some(measurer) if measurer.measures_for(&self.params) => measurer,
            _ => LineMeasurer::new(cx.text_system(), &self.params),
        }
    }

    /// Wraps the stale lines among buffer `rows` again, then, while it looks
    /// like they all fit in `budget`, the other stale lines. Returns whether
    /// any line was wrapped again.
    pub(crate) fn wrap_stale_rows(
        &mut self,
        rows: Range<usize>,
        budget: Duration,
        cx: &mut App,
    ) -> bool {
        if self.stale_rows == 0 {
            return false;
        }
        let mut measurer = self.take_measurer(cx);
        let metrics = self.inline_metrics.clone();
        let changed = self._wrap_stale_rows(rows, budget, &mut |line, width, start| {
            measurer.wrap(line, width, start, &metrics)
        });
        self.measurer = Some(measurer);
        changed
    }

    fn _wrap_stale_rows<F>(&mut self, rows: Range<usize>, budget: Duration, wrap_line: &mut F) -> bool
    where
        F: FnMut(&str, Pixels, usize) -> Vec<gpui::Boundary>,
    {
        let started = Instant::now();
        let wrapped = self.wrap_stale_runs(rows, None, wrap_line);
        if self.stale_rows == 0 {
            return wrapped > 0;
        }
        // Lines take about as long as the ones just wrapped: go on with the
        // rest only if they look like they fit, and stop where they don't.
        let elapsed = started.elapsed();
        let fits = wrapped > 0
            && elapsed < budget
            && (elapsed / wrapped as u32).saturating_mul(self.stale_rows as u32) < budget - elapsed;
        let more = if fits {
            self.wrap_stale_runs(0..self.lines_count(), Some(started + budget), wrap_line)
        } else {
            0
        };
        wrapped + more > 0
    }

    /// How many of the lines `rows` are stale.
    fn stale_rows_in(&self, rows: Range<usize>) -> usize {
        if self.stale_rows == 0 {
            return 0;
        }
        let mut cursor = self.lines.cursor::<BufferRows>(&());
        cursor.seek(&BufferRows(rows.start), Bias::Right);
        let mut stale = 0;
        for _ in rows {
            let Some(line) = cursor.item() else {
                break;
            };
            stale += usize::from(line.generation != self.generation);
            cursor.next();
        }
        stale
    }

    /// Wraps the stale lines among `rows` again, stopping at `deadline`.
    /// Returns how many it wrapped.
    fn wrap_stale_runs<F>(
        &mut self,
        rows: Range<usize>,
        deadline: Option<Instant>,
        wrap_line: &mut F,
    ) -> usize
    where
        F: FnMut(&str, Pixels, usize) -> Vec<gpui::Boundary>,
    {
        let rows = rows.start.min(self.lines_count())..rows.end.min(self.lines_count());
        let mut runs: Vec<Range<usize>> = Vec::new();
        let mut cursor = self.lines.cursor::<BufferRows>(&());
        cursor.seek(&BufferRows(rows.start), Bias::Right);
        for row in rows {
            let Some(line) = cursor.item() else {
                break;
            };
            if line.generation != self.generation {
                match runs.last_mut() {
                    Some(run) if run.end == row => run.end += 1,
                    _ => runs.push(row..row + 1),
                }
            }
            cursor.next();
        }
        drop(cursor);

        let mut wrapped = 0;
        for run in runs {
            let mut new_lines = Vec::with_capacity(run.len());
            for (line, start) in text_lines(&self.text, run.clone()) {
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    break;
                }
                new_lines.push(wrap_line_item(
                    line,
                    start,
                    &self.params,
                    self.generation,
                    wrap_line,
                ));
            }
            let done = new_lines.len();
            if done == 0 {
                break;
            }
            self.replace_rows(run.start..run.start + done, new_lines);
            wrapped += done;
            if done < run.len() {
                break;
            }
        }
        wrapped
    }

    /// Replaces the lines `rows` with `new_lines`, keeping count of the
    /// stale ones.
    fn replace_rows(&mut self, rows: Range<usize>, new_lines: Vec<LineItem>) {
        let stale = new_lines
            .iter()
            .filter(|line| line.generation != self.generation)
            .count();
        self.stale_rows = self.stale_rows - self.stale_rows_in(rows.clone()) + stale;
        splice_rows(&mut self.lines, rows, new_lines);
    }

    /// Records that a background wrap with the current parameters is to
    /// start, unless no line is stale or one is to already. Returns whether
    /// it is.
    pub(crate) fn schedule_background_wrap(&mut self) -> bool {
        if self.stale_rows == 0
            || self
                .background
                .as_ref()
                .is_some_and(|(generation, _)| *generation == self.generation)
        {
            return false;
        }
        self.background = Some((self.generation, Vec::new()));
        true
    }

    /// Starts the scheduled background wrap, of the lines stale now: the
    /// edits made from here on are applied to what it wraps. Returns whether
    /// it starts: not once it was dropped, or no line is stale any more.
    fn start_background(&mut self) -> bool {
        let Some((_, edits)) = &mut self.background else {
            return false;
        };
        if self.stale_rows == 0 {
            self.background = None;
            return false;
        }
        edits.clear();
        true
    }

    /// The scheduled background wrap, see [`Self::start_background`].
    pub(crate) fn background_wrap(&mut self, cx: &App) -> Option<WrapJob> {
        self.start_background().then(|| WrapJob {
            text: self.text.clone(),
            lines: self.lines.clone(),
            params: self.params.clone(),
            inline_metrics: self.inline_metrics.clone(),
            generation: self.generation,
            text_system: cx.text_system().clone(),
        })
    }

    /// Takes the lines a background wrap made, with the edits made since it
    /// started applied to them, unless the parameters changed since. Returns
    /// whether it took them.
    pub(crate) fn apply_background_wrap(&mut self, wrapped: WrappedLines) -> bool {
        let Some((_, edits)) = self
            .background
            .take_if(|(generation, _)| *generation == wrapped.generation)
        else {
            return false;
        };
        let mut lines = wrapped.lines;
        let mut stale_since = false;
        for (rows, new_lines) in edits {
            stale_since |= new_lines
                .iter()
                .any(|line| line.generation != self.generation);
            splice_rows(&mut lines, rows, new_lines);
        }
        debug_assert_eq!(lines.summary().buffer_rows, self.lines_count());
        self.lines = lines;
        // Only a whole text replaced since leaves lines stale.
        self.stale_rows = if stale_since {
            self.lines
                .iter()
                .filter(|line| line.generation != self.generation)
                .count()
        } else {
            0
        };
        true
    }

    /// Lays the text out for the first time: see [`Self::reset_lines`].
    pub(crate) fn prepare_if_need(&mut self, text: &Rope) -> bool {
        if self._initialized {
            return false;
        }
        self._initialized = true;
        self.text = text.clone();
        self.reset_lines();
        true
    }

    /// Update the text wrapper and recalculate the wrapped lines.
    ///
    /// - `changed_text`: The text [`Rope`] that has changed.
    /// - `range`: The `selected_range` before change.
    /// - `new_len`: The length of the inserted text.
    /// - `cx`: The application context.
    pub(crate) fn update(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_len: usize,
        cx: &mut App,
    ) {
        let mut measurer = self.take_measurer(cx);
        let metrics = self.inline_metrics.clone();
        self._update(
            changed_text,
            range,
            new_len,
            &mut |line_str, wrap_width, line_start| {
                measurer.wrap(line_str, wrap_width, line_start, &metrics)
            },
        );
        self.measurer = Some(measurer);
    }

    pub(crate) fn adjust_inline_metrics(&mut self, range: &Range<usize>, new_len: usize) {
        if self.inline_metrics.is_empty() {
            return;
        }
        let shift = new_len as isize - range.len() as isize;
        self.inline_metrics = self
            .inline_metrics
            .iter()
            .filter_map(|(token, width)| {
                if token.start < range.end && range.start < token.end {
                    return None;
                }
                let token = if token.start >= range.end {
                    token.start.checked_add_signed(shift)?..token.end.checked_add_signed(shift)?
                } else {
                    token.clone()
                };
                Some((token, *width))
            })
            .collect();
    }

    pub(crate) fn set_inline_metrics(
        &mut self,
        metrics: Arc<[(Range<usize>, Pixels)]>,
        cx: &mut App,
    ) {
        if self.inline_metrics == metrics {
            return;
        }
        // Only rows whose element geometry changed need another wrap pass.
        let mut affected = Vec::new();
        let (mut old, mut new) = (
            self.inline_metrics.iter().peekable(),
            metrics.iter().peekable(),
        );
        while old.peek().is_some() || new.peek().is_some() {
            match (old.peek(), new.peek()) {
                (Some(a), Some(b)) if a == b => {
                    old.next();
                    new.next();
                }
                (Some(a), Some(b)) if a.0.start <= b.0.start => {
                    affected.push(a.0.clone());
                    old.next();
                }
                (Some(_), Some(b)) | (None, Some(b)) => {
                    affected.push(b.0.clone());
                    new.next();
                }
                (Some(a), None) => {
                    affected.push(a.0.clone());
                    old.next();
                }
                (None, None) => break,
            }
        }
        self.inline_metrics = metrics;
        let text = self.text.clone();
        let mut rows: Vec<usize> = affected
            .iter()
            .map(|r| text.offset_to_point(r.start.min(text.len())).row)
            .collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            let start = text.line_start_offset(row);
            let end = text.line_end_offset(row);
            self.update(&text, &(start..end), end - start, cx);
        }
    }

    fn _update<F>(
        &mut self,
        changed_text: &Rope,
        range: &Range<usize>,
        new_len: usize,
        wrap_line: &mut F,
    ) where
        F: FnMut(&str, Pixels, usize) -> Vec<gpui::Boundary>,
    {
        if self.params.wrap_width.is_some()
            && new_len > SYNC_WRAP_BYTES
            && range.start == 0
            && range.end >= self.text.len()
        {
            // A new text, such as a file opened, is wrapped where it is seen.
            self.text = changed_text.clone();
            self.reset_lines();
            return;
        }

        // Remove the old changed lines.
        let buffer_line_count = self.lines_count();
        let start_row = self.text.offset_to_point(range.start).row;
        let start_row = start_row.min(buffer_line_count.saturating_sub(1));
        let end_row = self.text.offset_to_point(range.end).row;
        let end_row = end_row.min(buffer_line_count.saturating_sub(1));

        // To add the new lines.
        let new_start_row = changed_text.offset_to_point(range.start).row;
        let new_end_row = changed_text.offset_to_point(range.start + new_len).row;

        let new_lines: Vec<LineItem> = text_lines(changed_text, new_start_row..new_end_row + 1)
            .map(|(line, start)| {
                wrap_line_item(line, start, &self.params, self.generation, wrap_line)
            })
            .collect();

        if let Some((_, edits)) = &mut self.background {
            edits.push((start_row..end_row + 1, new_lines.clone()));
        }
        self.replace_rows(start_row..end_row + 1, new_lines);

        self.text = changed_text.clone();
    }

    /// Return display point (with soft wrap) from the given byte offset in the text.
    ///
    /// Panics if the `offset` is out of bounds.
    pub(crate) fn offset_to_display_point(&self, offset: usize) -> WrapDisplayPoint {
        self.offset_to_display_point_with_affinity(offset, false)
    }

    /// Like [`Self::offset_to_display_point`], but honours the caret's line-end affinity.
    ///
    /// A soft wrap boundary is one offset shared by two visual rows. Without affinity it always
    /// resolves to the start of the second row, which is wrong for a caret that is being drawn at
    /// the end of the first one -- vertical movement would then step from the row below the one
    /// the user can see.
    pub(crate) fn offset_to_display_point_with_affinity(
        &self,
        offset: usize,
        line_end_affinity: bool,
    ) -> WrapDisplayPoint {
        let row = self.text.offset_to_point(offset).row;
        let start = self.text.line_start_offset(row);

        // Seek to buffer row
        let mut cursor = self.lines.cursor::<Dimensions<BufferRows, WrapRows>>(&());
        cursor.seek(&BufferRows(row), Bias::Right);
        let wrapped_row = cursor.start().1.0;
        let Some(line) = cursor.item() else {
            return WrapDisplayPoint::new(wrapped_row, 0, 0);
        };

        let local_offset = offset.saturating_sub(start);
        for (ix, range) in line.wrapped_lines.iter().enumerate() {
            // With affinity the boundary offset closes the current row instead of opening the
            // next one, so the range is matched inclusively.
            let matches =
                range.contains(&local_offset) || (line_end_affinity && local_offset == range.end);
            if matches {
                return WrapDisplayPoint::new(
                    wrapped_row + ix,
                    ix,
                    local_offset.saturating_sub(range.start),
                );
            }
        }

        // Otherwise return the eof of the line.
        let last_range = line.wrapped_lines.last().unwrap_or(&(0..0));
        let ix = line.lines_len().saturating_sub(1);
        return WrapDisplayPoint::new(wrapped_row + ix, ix, last_range.len());
    }

    /// Return byte offset in the text from the given display point (with soft wrap).
    ///
    /// Panics if the `point.row` is out of bounds.
    pub(crate) fn display_point_to_offset(&self, point: WrapDisplayPoint) -> usize {
        // Seek to wrap row `point.row`
        let mut cursor = self.lines.cursor::<Dimensions<WrapRows, BufferRows>>(&());
        cursor.seek(&WrapRows(point.row), Bias::Right);
        let Some(line) = cursor.item() else {
            return self.text.len();
        };
        let wrapped_row = cursor.start().0.0;
        let row = cursor.start().1.0;

        let line_start = self.text.line_start_offset(row);
        let local_row = point.row.saturating_sub(wrapped_row);
        if let Some(range) = line.wrapped_lines.get(local_row) {
            line_start + (range.start + point.column).min(range.end)
        } else {
            // If not found, return the end of the line.
            line_start + line.len()
        }
    }

    pub(crate) fn display_point_to_point(&self, point: WrapDisplayPoint) -> TreeSitterPoint {
        let offset = self.display_point_to_offset(point);
        self.text.offset_to_point(offset)
    }

    pub(crate) fn point_to_display_point(&self, point: TreeSitterPoint) -> WrapDisplayPoint {
        let offset = self.text.point_to_offset(point);
        self.offset_to_display_point(offset)
    }
}

/// A display point within the soft-wrapped text.
///
/// This represents a position in the text after soft-wrapping,
/// with an additional `local_row` field tracking the wrap line
/// within the original buffer line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WrapDisplayPoint {
    /// The 0-based soft wrapped row index in the text.
    pub row: usize,
    /// The 0-based row index in local line (include first line).
    ///
    /// This value only valid when return from [`TextWrapper::offset_to_display_point`], otherwise it will be ignored.
    pub local_row: usize,
    /// The 0-based column byte index in the display line (with soft wrap).
    pub column: usize,
}

impl WrapDisplayPoint {
    pub(crate) fn new(row: usize, local_row: usize, column: usize) -> Self {
        Self {
            row,
            local_row,
            column,
        }
    }
}

/// The layout info of a line with soft wrapped lines.
pub(crate) struct LineLayout {
    /// Total bytes length of this line.
    len: usize,
    /// The soft wrapped lines of this line (Include the first line).
    pub(crate) wrapped_lines: SmallVec<[InputLine; 1]>,
    /// Extra left offset applied to continuation wrapped lines, used to reserve the first line's
    /// indentation when [`WrappingIndent::Same`] is used.
    pub(crate) wrap_indent: Pixels,
    pub(crate) longest_width: Pixels,
    pub(crate) whitespace_indicators: Option<WhitespaceIndicators>,
    /// Whitespace indicators: (line_index, x_position, is_tab)
    pub(crate) whitespace_chars: Vec<(usize, Pixels, bool)>,
    /// Whether any run of this line carries a background color, so [`Self::paint_background`]
    /// can skip the glyph walk for the common case of a line without highlights.
    has_background: bool,
}

impl LineLayout {
    pub(crate) fn new() -> Self {
        Self {
            len: 0,
            longest_width: px(0.),
            wrapped_lines: SmallVec::new(),
            wrap_indent: px(0.),
            whitespace_chars: Vec::new(),
            whitespace_indicators: None,
            has_background: false,
        }
    }

    /// Record whether any run of this line carries a background color.
    pub(crate) fn with_background(mut self, has_background: bool) -> Self {
        self.has_background = has_background;
        self
    }

    /// Set the left offset reserved for continuation wrapped lines.
    pub(crate) fn wrap_indent(mut self, wrap_indent: Pixels) -> Self {
        self.wrap_indent = wrap_indent;
        self
    }

    /// The pixel indent applied to the given visual line, relative to the line's
    /// leading text. Only continuation lines (index > 0) are indented.
    #[inline]
    fn line_indent(&self, line_index: usize) -> Pixels {
        if line_index == 0 {
            px(0.)
        } else {
            self.wrap_indent
        }
    }

    pub(crate) fn lines(mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) -> Self {
        self.set_wrapped_lines(wrapped_lines);
        self
    }

    pub(crate) fn set_wrapped_lines(&mut self, wrapped_lines: SmallVec<[ShapedLine; 1]>) {
        self.len = wrapped_lines.iter().map(|l| l.len).sum();
        let width = wrapped_lines
            .iter()
            .map(|l| l.width)
            .max()
            .unwrap_or_default();
        self.longest_width = width;
        self.wrapped_lines = wrapped_lines.into_iter().map(InputLine::from).collect();
    }

    pub(crate) fn inline_lines(mut self, lines: SmallVec<[InputLine; 1]>) -> Self {
        self.len = lines.iter().map(|line| line.len).sum();
        self.longest_width = lines
            .iter()
            .map(|line| line.width)
            .max()
            .unwrap_or_default();
        self.wrapped_lines = lines;
        self
    }

    pub(crate) fn with_whitespaces(mut self, indicators: Option<WhitespaceIndicators>) -> Self {
        self.whitespace_indicators = indicators;
        let Some(indicators) = self.whitespace_indicators.as_ref() else {
            return self;
        };

        let space_indicator_offset = indicators.space.width.half();

        for (line_index, wrapped_line) in self.wrapped_lines.iter().enumerate() {
            for (text_offset, c) in wrapped_line.text.char_indices() {
                let relative_offset = wrapped_line.text_offset() + text_offset;
                if matches!(c, ' ' | '\t') {
                    let is_tab = c == '\t';
                    let start_x = wrapped_line.x_for_index(relative_offset);
                    let end_x = wrapped_line.x_for_index(relative_offset + c.len_utf8());
                    // Center the indicator in the actual character's space
                    let x_position = if c == ' ' {
                        (start_x + end_x).half() - space_indicator_offset
                    } else {
                        start_x
                    };

                    self.whitespace_chars.push((line_index, x_position, is_tab));
                }
            }
        }
        self
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// Get the position (x, y) for the given index in this line layout.
    ///
    /// - The `offset` is a local byte index in this line layout.
    /// - When `line_end_affinity` is true, an offset at a soft wrap boundary is placed at
    ///   the end of the current visual line rather than the start of the next one.
    /// - The return value is relative to the top-left corner of this line layout, start from (0, 0)
    pub(crate) fn position_for_index(
        &self,
        offset: usize,
        last_layout: &LastLayout,
        line_end_affinity: bool,
    ) -> Option<Point<Pixels>> {
        let mut acc_len = 0;
        let mut offset_y = px(0.);

        let x_offset = last_layout.alignment_offset(self.longest_width);

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let is_last = i + 1 == self.wrapped_lines.len();

            let matches = if line.len == 0 {
                // Empty visual lines still own their boundary offset.
                offset == acc_len
            } else if is_last || line_end_affinity {
                // Inclusive: cursor can sit at end of this visual line.
                offset >= acc_len && offset <= acc_len + line.len
            } else {
                // Exclusive: boundary offset belongs to the next visual line.
                offset >= acc_len && offset < acc_len + line.len
            };

            if matches {
                let x = line.x_for_index(offset.saturating_sub(acc_len))
                    + x_offset
                    + self.line_indent(i);
                return Some(point(x, offset_y));
            }

            // Always advance by actual line length. The last line gets +1 so the
            // cursor can be placed after the final character.
            acc_len += if is_last { line.len + 1 } else { line.len };
            offset_y += last_layout.line_height;
        }

        None
    }

    /// Get the closest index for the given x in this line layout.
    ///
    /// This ignores y, so it only makes sense for a layout that is known to occupy a single
    /// visual line. Wrapped layouts must use [`Self::closest_index_for_position`], which also
    /// reports the caret affinity that a wrap boundary needs.
    pub(crate) fn closest_index_for_x(&self, x: Pixels, last_layout: &LastLayout) -> usize {
        let mut acc_len = 0;
        let x_offset = last_layout.alignment_offset(self.longest_width);
        let x = x - x_offset;

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let line_indent = self.line_indent(i);
            if x <= line_indent + line.width {
                return acc_len + line.closest_index_for_x(x - line_indent);
            }
            acc_len += line.len;
        }

        acc_len
    }

    /// Resolve `pos` to the wrapped sub-line under it.
    ///
    /// Returns the sub-line index, the byte offset that sub-line starts at within this line
    /// layout, and `pos.x` translated into that sub-line's own coordinate space.
    fn wrapped_line_at(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<(usize, usize, Pixels)> {
        let mut offset = 0;
        let mut line_top = px(0.);
        let x_offset = last_layout.alignment_offset(self.longest_width);

        for (i, line) in self.wrapped_lines.iter().enumerate() {
            let line_bottom = line_top + last_layout.line_height;
            if pos.y >= line_top && pos.y < line_bottom {
                return Some((i, offset, pos.x - x_offset - self.line_indent(i)));
            }

            offset += line.len;
            line_top = line_bottom;
        }

        None
    }

    /// Get the index for the given position (x, y) in this line layout.
    ///
    /// The `pos` is relative to the top-left corner of this line layout, start from (0, 0).
    ///
    /// Returns a local byte index in this line layout (start from 0) together with the caret
    /// affinity to use for it: `true` when the index landed on the wrap boundary of a non-final
    /// sub-line. That boundary offset is shared by the end of one visual line and the start of
    /// the next, so the affinity is what tells [`Self::position_for_index`] which of the two the
    /// caret belongs to. Without it a click past the last glyph of a wrapped line would put a
    /// visible caret on the following line.
    pub(crate) fn closest_index_for_position(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<(usize, bool)> {
        let (i, offset, x) = self.wrapped_line_at(pos, last_layout)?;
        let line = &self.wrapped_lines[i];
        let ix = line.closest_index_for_x(x);
        let line_end_affinity = i + 1 < self.wrapped_lines.len() && ix == line.len;

        Some((offset + ix, line_end_affinity))
    }

    /// How many columns the given position sits past the end of the line under it.
    ///
    /// Past the end of a line there is no glyph to hit-test against, so a position out
    /// there resolves to the line end and loses how far right it really was. The extra
    /// distance is reported here in whole spaces, letting a columnar selection keep its
    /// width over a short row. Only the final visual line of a wrapped layout has that
    /// trailing space; a continuation line ends at a wrap boundary, where the next glyph
    /// merely lives on the following row.
    ///
    /// The `pos` is relative to the top-left corner of this line layout, start from (0, 0).
    pub(crate) fn columns_past_line_end(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> usize {
        let Some((i, _, x)) = self.wrapped_line_at(pos, last_layout) else {
            return 0;
        };

        if i + 1 < self.wrapped_lines.len() || last_layout.space_width <= px(0.) {
            return 0;
        }

        let past_end = x - self.wrapped_lines[i].width;
        if past_end <= px(0.) {
            return 0;
        }

        (past_end / last_layout.space_width).round() as usize
    }

    pub(crate) fn index_for_position(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<usize> {
        let (i, offset, x) = self.wrapped_line_at(pos, last_layout)?;

        Some(offset + self.wrapped_lines[i].index_for_x(x)?)
    }

    pub(crate) fn size(&self, line_height: Pixels) -> Size<Pixels> {
        let width = self
            .wrapped_lines
            .iter()
            .enumerate()
            .map(|(ix, line)| line.width + self.line_indent(ix))
            .max()
            .unwrap_or(self.longest_width);
        size(width, self.wrapped_lines.len() * line_height)
    }

    /// Paint only the glyph background quads of this line.
    ///
    /// gpui's [`ShapedLine::paint`] does not draw backgrounds, so every line painted with
    /// [`Self::paint`] needs this called first, with the same origin and align width.
    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
        text_align: TextAlign,
        align_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Painting a background walks every glyph and pushes a scene layer, so skip the
        // whole pass for lines that have no background color to paint.
        if !self.has_background {
            return;
        }

        for (ix, line) in self.wrapped_lines.iter().enumerate() {
            _ = line.paint_background(
                pos + point(self.line_indent(ix), ix * line_height),
                line_height,
                text_align,
                align_width,
                window,
                cx,
            );
        }
    }

    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        line_height: Pixels,
        text_align: TextAlign,
        align_width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (ix, line) in self.wrapped_lines.iter().enumerate() {
            _ = line.paint(
                pos + point(self.line_indent(ix), ix * line_height),
                line_height,
                text_align,
                align_width,
                window,
                cx,
            );
        }

        // Paint whitespace indicators
        if let Some(indicators) = self.whitespace_indicators.as_ref() {
            for (line_index, x_position, is_tab) in &self.whitespace_chars {
                let invisible = if *is_tab {
                    indicators.tab.clone()
                } else {
                    indicators.space.clone()
                };

                let origin = point(
                    pos.x + *x_position + self.line_indent(*line_index),
                    pos.y + *line_index as f32 * line_height,
                );

                _ = invisible.paint(origin, line_height, text_align, align_width, window, cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[cfg(target_os = "linux")]
    use gpui::TestAppContext;
    use gpui::{Boundary, FontFeatures, FontStyle, FontWeight, px};

    #[cfg(target_os = "linux")]
    fn shaped_width(text: &str, font: &Font, font_size: Pixels, cx: &App) -> Pixels {
        gpui::WindowTextSystem::new(cx.text_system().clone())
            .layout_line(
                text,
                font_size,
                &[gpui::TextRun {
                    len: text.len(),
                    font: font.clone(),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                }],
                None,
            )
            .width
    }

    // Linux exposes its native text engine without creating a desktop window.
    // macOS platform creation requires the main thread, and Windows headless
    // mode uses NoopTextSystem rather than native shaping.
    #[cfg(target_os = "linux")]
    fn shaping_test_context() -> TestAppContext {
        let platform = gpui_platform::current_platform(true);
        TestAppContext::build_with_text_system(
            gpui::TestDispatcher::new(0),
            None,
            platform.text_system(),
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_wrap_keeps_shaped_cjk_latin_boundary_during_edits() {
        let cx = shaping_test_context();
        cx.update(|cx| {
            let font = test_font();
            let font_size = px(14.);
            let prefix = "abcd的";
            let width = shaped_width(prefix, &font, font_size, cx);
            assert!(shaped_width("abcd的s", &font, font_size, cx) > width);
            let mut wrapper = TextWrapper::new(font.clone(), font_size, Some(width));
            let mut previous = Rope::new();

            for value in ["abcd的", "abcd的s", "abcd的ss", "abcd的s", "abcd的"] {
                let text = Rope::from(value);
                let start = previous.len().min(text.len());
                wrapper.update(&text, &(start..previous.len()), text.len() - start, cx);
                let expected = if value.ends_with('s') {
                    vec![0..prefix.len(), prefix.len()..value.len()]
                } else {
                    vec![0..prefix.len()]
                };
                assert_eq!(wrapper.line(0).unwrap().wrapped_lines.as_slice(), expected);
                for range in &wrapper.line(0).unwrap().wrapped_lines {
                    assert!(shaped_width(&value[range.clone()], &font, font_size, cx) <= width);
                }
                assert_eq!(
                    wrapper.offset_to_display_point(value.len()).row,
                    usize::from(value.ends_with('s'))
                );
                previous = text;
            }
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_wrap_preserves_words_and_complete_graphemes() {
        let cx = shaping_test_context();
        cx.update(|cx| {
            let font = test_font();
            let font_size = px(14.);
            for (value, prefix) in [("hello world", "hello "), ("a👩‍💻b", "a👩‍💻")] {
                // Either complete row must fit even when the native font makes
                // the second word wider than the first word and its space.
                let width = shaped_width(prefix, &font, font_size, cx).max(shaped_width(
                    &value[prefix.len()..],
                    &font,
                    font_size,
                    cx,
                ));
                assert!(shaped_width(value, &font, font_size, cx) > width);
                let text = Rope::from(value);
                let mut wrapper = TextWrapper::new(font.clone(), font_size, Some(width));
                wrapper.update(&text, &(0..0), text.len(), cx);
                assert_eq!(
                    wrapper.line(0).unwrap().wrapped_lines.as_slice(),
                    [0..prefix.len(), prefix.len()..value.len()]
                );
                for range in &wrapper.line(0).unwrap().wrapped_lines {
                    assert!(shaped_width(&value[range.clone()], &font, font_size, cx) <= width);
                }
            }
        });
    }

    #[test]
    fn measured_wrap_keeps_cjk_latin_boundary_stable_during_edits() {
        let measure = |text: &str| {
            px(text
                .chars()
                .map(|c| if c.is_ascii() { 1. } else { 2. })
                .sum())
        };
        let mut wrapper = TextWrapper::new(gpui::font("Arial"), px(14.), Some(px(6.)));
        let mut previous = Rope::new();
        for value in ["abcd的", "abcd的s", "abcd的ss", "abcd的s", "abcd的"] {
            let text = Rope::from(value);
            let start = previous.len().min(text.len());
            wrapper._update(
                &text,
                &(start..previous.len()),
                text.len() - start,
                &mut |line, width, _| {
                    measured_wrap_boundaries(line, width, WrappingIndent::None, measure)
                },
            );
            let expected = if value.ends_with('s') {
                vec![0..7, 7..value.len()]
            } else {
                vec![0..7]
            };
            assert_eq!(wrapper.line(0).unwrap().wrapped_lines.as_slice(), expected);
            let cursor = wrapper.offset_to_display_point(value.len());
            assert_eq!(cursor.row, usize::from(value.ends_with('s')));
            previous = text;
        }
    }

    #[test]
    fn measured_wrap_preserves_words_graphemes_and_indentation() {
        let wrap = |text: &str, width, indent| {
            measured_wrap_boundaries(text, px(width), indent, |s| {
                px(s.graphemes(true).count() as f32)
            })
            .into_iter()
            .map(|b| b.ix)
            .collect::<Vec<_>>()
        };
        assert_eq!(wrap("hello world", 8., WrappingIndent::None), vec![6]);
        assert_eq!(wrap("a👩‍💻b", 2., WrappingIndent::None), vec!["a👩‍💻".len()]);
        assert_eq!(wrap("  abcdefgh", 5., WrappingIndent::Same), vec![5, 8]);
        assert_eq!(wrap("  abcdefgh", 5., WrappingIndent::None), vec![2, 7]);
        assert!(wrap("", 0., WrappingIndent::None).is_empty());
        assert_eq!(wrap("abc", 0., WrappingIndent::None), vec![1, 2]);
        // Closing punctuation stays with the preceding Chinese character.
        assert_eq!(wrap("你好，世界", 2., WrappingIndent::None), vec![3, 9]);
    }

    #[test]
    fn test_update() {
        let font = gpui::Font {
            family: "Arial".into(),
            weight: FontWeight::default(),
            style: FontStyle::Normal,
            features: FontFeatures::default(),
            fallbacks: None,
        };

        let mut wrapper = TextWrapper::new(font, px(14.), None);
        let mut text = Rope::from(
            "Hello, 世界!\r\nThis is second line.\nThis is third line.\n这里是第 4 行。",
        );

        fn fake_wrap_line(_line: &str, _wrap_width: Pixels, _: usize) -> Vec<Boundary> {
            vec![]
        }

        #[track_caller]
        fn assert_wrapper_lines(text: &Rope, wrapper: &TextWrapper, expected_lines: &[&[&str]]) {
            let mut actual_lines = vec![];
            let mut offset = 0;
            for line in wrapper.iter_lines() {
                actual_lines.push(
                    line.wrapped_lines
                        .iter()
                        .map(|range| text.slice(offset + range.start..offset + range.end))
                        .collect::<Vec<_>>(),
                );
                // +1 \n
                offset += line.len() + 1;
            }
            assert_eq!(actual_lines, expected_lines);
        }

        wrapper._update(&text, &(0..text.len()), text.len(), &mut fake_wrap_line);
        assert_eq!(wrapper.lines_count(), 4);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["Hello, 世界!\r"],
                &["This is second line."],
                &["This is third line."],
                &["这里是第 4 行。"],
            ],
        );

        // Add a new text to end
        let range = text.len()..text.len();
        let new_text = "New text";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "Hello, 世界!\r\nThis is second line.\nThis is third line.\n这里是第 4 行。New text"
        );
        assert_eq!(wrapper.lines_count(), 4);
        assert_eq!(wrapper.lines_count(), 4);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["Hello, 世界!\r"],
                &["This is second line."],
                &["This is third line."],
                &["这里是第 4 行。New text"],
            ],
        );

        // Replace first line `Hello` to `AAA`
        let range = 0..5;
        let new_text = "AAA";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "AAA, 世界!\r\nThis is second line.\nThis is third line.\n这里是第 4 行。New text"
        );
        assert_eq!(wrapper.lines_count(), 4);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["AAA, 世界!\r"],
                &["This is second line."],
                &["This is third line."],
                &["这里是第 4 行。New text"],
            ],
        );

        // Remove the second line
        let start_offset = text.line_start_offset(1);
        let end_offset = text.line_end_offset(1);
        let range = start_offset..end_offset + 1;
        text.replace(range.clone(), "");
        wrapper._update(&text, &range, 0, &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "AAA, 世界!\r\nThis is third line.\n这里是第 4 行。New text"
        );
        assert_eq!(wrapper.lines_count(), 3);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["AAA, 世界!\r"],
                &["This is third line."],
                &["这里是第 4 行。New text"],
            ],
        );

        // Replace the first 2 lines to "This is a new line."
        let range = text.line_start_offset(0)..text.line_end_offset(1) + 1;
        let new_text = "This is a new line.\nThis is new line 2.\n";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "This is a new line.\nThis is new line 2.\n这里是第 4 行。New text"
        );
        assert_eq!(wrapper.lines_count(), 3);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["This is a new line."],
                &["This is new line 2."],
                &["这里是第 4 行。New text"],
            ],
        );

        // Add a new line at the end
        let range = text.len()..text.len();
        let new_text = "\nThis is a new line at the end.";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "This is a new line.\nThis is new line 2.\n这里是第 4 行。New text\nThis is a new line at the end."
        );
        assert_eq!(wrapper.lines_count(), 4);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["This is a new line."],
                &["This is new line 2."],
                &["这里是第 4 行。New text"],
                &["This is a new line at the end."],
            ],
        );

        // Add a new line at the beginning
        let range = 0..0;
        let new_text = "This is a new line at the beginning.\n";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "This is a new line at the beginning.\nThis is a new line.\nThis is new line 2.\n这里是第 4 行。New text\nThis is a new line at the end."
        );
        assert_eq!(wrapper.lines_count(), 5);
        assert_wrapper_lines(
            &text,
            &wrapper,
            &[
                &["This is a new line at the beginning."],
                &["This is a new line."],
                &["This is new line 2."],
                &["这里是第 4 行。New text"],
                &["This is a new line at the end."],
            ],
        );

        // Remove all to at least one line in `lines`.
        let range = 0..text.len();
        let new_text = "";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);
        assert_eq!(text.to_string(), "");
        assert_eq!(wrapper.lines_count(), 1);
        assert_eq!(wrapper.line(0).unwrap().wrapped_lines.as_slice(), [0..0]);

        // Test update_all
        let range = 0..text.len();
        let new_text = "This is a full text.\nThis is a second line.";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, text.len(), &mut fake_wrap_line);
        assert_eq!(
            text.to_string(),
            "This is a full text.\nThis is a second line."
        );
        assert_eq!(wrapper.lines_count(), 2);
    }

    fn test_font() -> gpui::Font {
        gpui::Font {
            family: "Arial".into(),
            weight: FontWeight::default(),
            style: FontStyle::Normal,
            features: FontFeatures::default(),
            fallbacks: None,
        }
    }

    /// The longest-row summary stays exact when the previously-longest line is shrunk.
    #[test]
    fn test_longest_row_after_shrink() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), None);
        let mut text = Rope::from("aa\nthis is the longest line\nbb");
        wrapper._update(&text, &(0..text.len()), text.len(), &mut |_, _, _| vec![]);
        assert_eq!(wrapper.longest_row(), 1);

        // Shrink line 1 so line 2-equivalent isn't longest.
        // Make line 0 the longest now.
        let start = text.line_start_offset(0);
        let end = text.line_end_offset(0);
        let range = start..end;
        let new_text = "a very very long first line now";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut |_, _, _| vec![]);
        assert_eq!(wrapper.longest_row(), 0);
    }

    /// Wraps at `width` columns, one column a byte, as the tests' text is ASCII.
    fn wrap_columns(line: &str, width: Pixels, _: usize) -> Vec<Boundary> {
        let columns = (f32::from(width) as usize).max(1);
        (1..line.len().div_ceil(columns))
            .map(|row| Boundary {
                ix: row * columns,
                next_indent: 0,
            })
            .collect()
    }

    fn wrapped_rows(wrapper: &TextWrapper) -> Vec<Vec<Range<usize>>> {
        wrapper
            .iter_lines()
            .map(|line| line.wrapped_lines.to_vec())
            .collect()
    }

    fn with_wrap_width(wrapper: &TextWrapper, columns: f32) -> WrapParams {
        WrapParams {
            wrap_width: Some(px(columns)),
            ..wrapper.params().clone()
        }
    }

    /// After the wrap width changes, lines keep their earlier wrapping until
    /// they are wrapped again, and only the ones asked for are.
    #[test]
    fn test_stale_lines_keep_their_wrapping_until_wrapped_again() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), Some(px(10.)));
        let text = Rope::from("a".repeat(20) + "\n" + &"a".repeat(20) + "\n" + &"a".repeat(20));
        wrapper._update(&text, &(0..text.len()), text.len(), &mut wrap_columns);
        assert_eq!(wrapper.len(), 6);

        assert!(wrapper.set_params(with_wrap_width(&wrapper, 5.)));
        assert!(!wrapper.set_params(with_wrap_width(&wrapper, 5.)));
        assert_eq!(wrapper.stale_rows(), 3);
        assert_eq!(wrapper.len(), 6, "lines keep their wrapping meanwhile");

        // No time for more than the line asked for.
        assert!(wrapper._wrap_stale_rows(1..2, Duration::ZERO, &mut wrap_columns));
        assert_eq!(wrapper.stale_rows(), 2);
        assert_eq!(wrapper.line(1).unwrap().lines_len(), 4);
        assert_eq!(wrapper.line(0).unwrap().lines_len(), 2);
        assert_eq!(wrapper.len(), 8);
        // Wrapping it again is a no-op.
        assert!(!wrapper._wrap_stale_rows(1..2, Duration::ZERO, &mut wrap_columns));

        // An edit wraps the lines it makes with the new width.
        let start = text.line_start_offset(2);
        let mut edited = text.clone();
        edited.replace(start..start + 1, "b");
        wrapper._update(&edited, &(start..start + 1), 1, &mut wrap_columns);
        assert_eq!(wrapper.stale_rows(), 1);
        assert_eq!(wrapper.line(2).unwrap().lines_len(), 4);

        // Lines already wrapped tell nothing of how long the rest would take:
        // they are left for the background.
        assert!(!wrapper._wrap_stale_rows(1..3, Duration::from_secs(60), &mut wrap_columns));
        assert_eq!(wrapper.stale_rows(), 1);
        // With time to spare after wrapping a line asked for, the rest is wrapped too.
        let mut text = edited;
        text.replace(0..0, "a\n");
        wrapper._update(&text, &(0..0), 2, &mut wrap_columns);
        assert!(wrapper.set_params(with_wrap_width(&wrapper, 4.)));
        assert!(wrapper._wrap_stale_rows(0..1, Duration::from_secs(60), &mut wrap_columns));
        assert_eq!(wrapper.stale_rows(), 0);
        assert_eq!(wrapper.len(), 1 + 3 * 5);
    }

    /// A background wrap's lines, with the edits made while it ran applied to
    /// them, are what wrapping everything at once gives; once the wrap width
    /// changes again they are refused.
    #[test]
    fn test_background_wrap_takes_the_edits_made_meanwhile() {
        let line = "a".repeat(20);
        let mut text = Rope::from([line.as_str(); 6].join("\n"));
        let mut wrapper = TextWrapper::new(test_font(), px(14.), Some(px(10.)));
        wrapper._update(&text, &(0..text.len()), text.len(), &mut wrap_columns);

        assert!(wrapper.set_params(with_wrap_width(&wrapper, 5.)));
        assert!(wrapper._wrap_stale_rows(4..5, Duration::ZERO, &mut wrap_columns));
        assert!(wrapper.schedule_background_wrap());
        assert!(!wrapper.schedule_background_wrap(), "one at a time");
        // Edits before it starts are in what it wraps.
        let range = text.line_start_offset(2)..text.line_start_offset(2) + 1;
        text.replace(range.clone(), "b");
        wrapper._update(&text, &range, 1, &mut wrap_columns);
        assert!(wrapper.start_background());
        let snapshot = text.clone();
        let snapshot_lines = wrapper.lines.clone();
        let params = wrapper.params().clone();
        let generation = wrapper.generation;

        // Edits while it runs: a line split in two, and one removed.
        for (range, new_text) in [
            (text.line_start_offset(1) + 3..text.line_start_offset(1) + 3, "bbbbbbb\nc"),
            (text.line_start_offset(4)..text.line_start_offset(5), ""),
        ] {
            text.replace(range.clone(), new_text);
            wrapper._update(&text, &range, new_text.len(), &mut wrap_columns);
        }

        let lines = wrap_all_lines(
            &snapshot,
            &snapshot_lines,
            &params,
            generation,
            &AtomicBool::new(false),
            &mut wrap_columns,
        )
        .unwrap();
        assert!(wrapper.apply_background_wrap(WrappedLines {
            generation,
            lines: lines.clone(),
        }));
        assert_eq!(wrapper.stale_rows(), 0);
        let mut fresh = TextWrapper::new(test_font(), px(14.), Some(px(5.)));
        fresh._update(&text, &(0..text.len()), text.len(), &mut wrap_columns);
        assert_eq!(wrapped_rows(&wrapper), wrapped_rows(&fresh));
        assert_eq!(wrapper.len(), fresh.len());

        // A wrap started before the width changed again is refused.
        assert!(wrapper.set_params(with_wrap_width(&wrapper, 7.)));
        assert!(wrapper.schedule_background_wrap());
        assert!(wrapper.start_background());
        assert!(!wrapper.apply_background_wrap(WrappedLines { generation, lines }));
        assert_eq!(wrapper.stale_rows(), wrapper.lines_count());

        // A cancelled one gives nothing.
        assert!(
            wrap_all_lines(
                &text,
                &wrapper.lines,
                &params,
                generation,
                &AtomicBool::new(true),
                &mut wrap_columns
            )
            .is_none()
        );
    }

    /// A background wrap leaves the lines wrapped already as they are, and
    /// wraps the stale ones only.
    #[test]
    fn test_background_wrap_wraps_the_stale_lines_only() {
        let text = Rope::from(["a".repeat(20).as_str(); 4].join("\n"));
        let mut wrapper = TextWrapper::new(test_font(), px(14.), Some(px(10.)));
        wrapper._update(&text, &(0..text.len()), text.len(), &mut wrap_columns);
        assert!(wrapper.set_params(with_wrap_width(&wrapper, 5.)));
        assert!(wrapper._wrap_stale_rows(1..3, Duration::ZERO, &mut wrap_columns));
        assert_eq!(wrapper.stale_rows(), 2);

        let mut wrapped = Vec::new();
        let lines = wrap_all_lines(
            &text,
            &wrapper.lines,
            wrapper.params(),
            wrapper.generation,
            &AtomicBool::new(false),
            &mut |line: &str, width, start| {
                wrapped.push(start);
                wrap_columns(line, width, start)
            },
        )
        .unwrap();
        assert_eq!(wrapped, [0, 3 * 21]);
        let rows = |lines: &SumTree<LineItem>| {
            lines
                .iter()
                .map(|line| line.wrapped_lines.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&lines), [4, 4, 4, 4]);
    }

    /// A new text replacing the whole of a long one is laid out a row a line
    /// in one pass, its lines left stale to wrap where they are seen; without
    /// a wrap width they are as they will stay at once.
    #[test]
    fn test_a_new_long_text_is_left_to_wrap_where_it_is_seen() {
        let line = "a".repeat(99);
        let text = Rope::from(vec![line.as_str(); SYNC_WRAP_BYTES / 100 + 1].join("\n"));
        let lines = text.lines_len();
        let mut wrapper = TextWrapper::new(test_font(), px(14.), Some(px(10.)));
        let mut wrapped = 0;
        wrapper._update(&text, &(0..0), text.len(), &mut |line, width, start| {
            wrapped += 1;
            wrap_columns(line, width, start)
        });
        assert_eq!(wrapped, 0);
        assert_eq!(wrapper.lines_count(), lines);
        assert_eq!(wrapper.len(), lines);
        assert_eq!(wrapper.stale_rows(), lines);
        assert!(wrapper._wrap_stale_rows(0..2, Duration::ZERO, &mut wrap_columns));
        assert_eq!(wrapper.stale_rows(), lines - 2);
        assert_eq!(wrapper.len(), lines + 2 * 9);

        assert!(wrapper.set_params(WrapParams {
            wrap_width: None,
            ..wrapper.params().clone()
        }));
        assert_eq!(wrapper.stale_rows(), 0);
        assert_eq!(wrapper.len(), lines);
    }

    /// Editing the last line and deleting everything must keep the tree consistent.
    #[test]
    fn test_edit_last_line_and_full_delete() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), None);
        let mut text = Rope::from("one\ntwo\nthree");
        wrapper._update(&text, &(0..text.len()), text.len(), &mut |_, _, _| vec![]);
        assert_eq!(wrapper.lines_count(), 3);

        // Replace the last line only.
        let start = text.line_start_offset(2);
        let range = start..text.len();
        let new_text = "THREE EDITED";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut |_, _, _| vec![]);
        assert_eq!(wrapper.lines_count(), 3);
        assert_eq!(wrapper.line(2).unwrap().len(), "THREE EDITED".len());

        // Delete everything.
        let range = 0..text.len();
        text.replace(range.clone(), "");
        wrapper._update(&text, &range, 0, &mut |_, _, _| vec![]);
        assert_eq!(wrapper.lines_count(), 1);
        assert_eq!(wrapper.len(), 1);
        assert_eq!(wrapper.line(0).unwrap().wrapped_lines.as_slice(), [0..0]);
    }

    #[test]
    fn test_wrap_row_buffer_line_boundaries() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), None);
        wrapper.text = Rope::from("aa\nbbbb\nc");
        wrapper.lines = SumTree::from_iter(
            vec![
                LineItem {
                    len: 2,
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..2],
                    generation: 0,
                },
                LineItem {
                    len: 4,
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..2, 2..4],
                    generation: 0,
                },
                LineItem {
                    len: 1,
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..1],
                    generation: 0,
                },
            ],
            &(),
        );

        assert_eq!(wrapper.lines_count(), 3);
        assert_eq!(wrapper.len(), 4);

        assert_eq!(wrapper.buffer_line_to_first_wrap_row(0), 0);
        assert_eq!(wrapper.buffer_line_to_first_wrap_row(1), 1);
        assert_eq!(wrapper.buffer_line_to_first_wrap_row(2), 3);
        assert_eq!(wrapper.buffer_line_to_first_wrap_row(3), 4);

        assert_eq!(wrapper.buffer_line_to_wrap_row_range(0), 0..1);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(1), 1..3);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(2), 3..4);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(3), 4..4);

        assert_eq!(wrapper.wrap_row_to_buffer_line(0), 0);
        assert_eq!(wrapper.wrap_row_to_buffer_line(1), 1);
        assert_eq!(wrapper.wrap_row_to_buffer_line(2), 1);
        assert_eq!(wrapper.wrap_row_to_buffer_line(3), 2);
        assert_eq!(wrapper.wrap_row_to_buffer_line(4), 2);
    }

    #[test]
    fn test_wrap_row_queries_after_incremental_splice() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), Some(px(10.)));
        let mut text = Rope::from("aa\nbbbb\nc");
        let mut fake_wrap_line = |line: &str, _wrap_width: Pixels, _: usize| {
            if line.len() > 2 {
                vec![Boundary {
                    ix: 2,
                    next_indent: 0,
                }]
            } else {
                vec![]
            }
        };

        wrapper._update(&text, &(0..text.len()), text.len(), &mut fake_wrap_line);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(0), 0..1);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(1), 1..3);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(2), 3..4);

        let range = text.line_start_offset(1)..text.line_end_offset(1);
        let new_text = "dd\neeee";
        text.replace(range.clone(), new_text);
        wrapper._update(&text, &range, new_text.len(), &mut fake_wrap_line);

        assert_eq!(wrapper.lines_count(), 4);
        assert_eq!(wrapper.len(), 5);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(0), 0..1);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(1), 1..2);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(2), 2..4);
        assert_eq!(wrapper.buffer_line_to_wrap_row_range(3), 4..5);
        assert_eq!(wrapper.wrap_row_to_buffer_line(0), 0);
        assert_eq!(wrapper.wrap_row_to_buffer_line(1), 1);
        assert_eq!(wrapper.wrap_row_to_buffer_line(2), 2);
        assert_eq!(wrapper.wrap_row_to_buffer_line(3), 2);
        assert_eq!(wrapper.wrap_row_to_buffer_line(4), 3);
    }

    #[test]
    fn test_line_layout() {
        let mut line_layout = LineLayout::new();

        let line1 = ShapedLine::default().with_len(100);
        let line2 = ShapedLine::default().with_len(50);
        let wrapped_lines = smallvec::smallvec![line1, line2];
        line_layout.set_wrapped_lines(wrapped_lines);
        assert_eq!(line_layout.len(), 150);
        assert_eq!(line_layout.wrapped_lines.len(), 2);
    }

    /// A layout context whose only load-bearing field is the line height.
    fn test_last_layout(line_height: Pixels) -> LastLayout {
        LastLayout {
            visible_range: 0..1,
            visible_buffer_lines: vec![0],
            visible_line_byte_offsets: vec![0],
            visible_top: px(0.),
            visible_range_offset: 0..0,
            lines: Rc::new(vec![]),
            line_height,
            line_number_width: px(0.),
            space_width: px(0.),
            cursor_bounds: None,
            text_align: TextAlign::Left,
            content_width: px(0.),
        }
    }

    #[test]
    fn test_position_for_index_prefers_first_leading_empty_visual_line() {
        let mut line_layout = LineLayout::new();
        line_layout.set_wrapped_lines(smallvec::smallvec![
            ShapedLine::default(),
            ShapedLine::default(),
            ShapedLine::default().with_len(3),
        ]);

        assert_eq!(
            line_layout.position_for_index(0, &test_last_layout(px(20.)), false),
            Some(point(px(0.), px(0.)))
        );
    }

    #[test]
    fn clicking_past_a_wrapped_row_keeps_the_caret_on_that_row() {
        // One buffer line wrapped into two visual rows, splitting at byte 10.
        let mut line_layout = LineLayout::new();
        line_layout.set_wrapped_lines(smallvec::smallvec![
            ShapedLine::default().with_len(10),
            ShapedLine::default().with_len(5),
        ]);
        let last_layout = test_last_layout(px(20.));

        // Clicking past the last glyph of the first row resolves to the wrap boundary, which is
        // also the first offset of the second row -- hence the affinity.
        let (ix, line_end_affinity) = line_layout
            .closest_index_for_position(point(px(999.), px(5.)), &last_layout)
            .unwrap();
        assert_eq!(ix, 10);
        assert!(line_end_affinity);

        // Carrying that affinity is what keeps the caret on the row that was clicked; dropping it
        // is the bug -- the caret shows up one row below the pointer.
        assert_eq!(
            line_layout
                .position_for_index(ix, &last_layout, line_end_affinity)
                .map(|pos| pos.y),
            Some(px(0.))
        );
        assert_eq!(
            line_layout
                .position_for_index(ix, &last_layout, false)
                .map(|pos| pos.y),
            Some(px(20.))
        );

        // The final row owns the end of the line outright, so there is nothing to disambiguate.
        let (ix, line_end_affinity) = line_layout
            .closest_index_for_position(point(px(999.), px(25.)), &last_layout)
            .unwrap();
        assert_eq!(ix, 15);
        assert!(!line_end_affinity);
    }

    #[test]
    fn a_wrap_boundary_offset_resolves_to_the_row_the_caret_is_drawn_on() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.), None);
        wrapper.text = Rope::from("first line\nthis one wraps");
        wrapper.lines = SumTree::from_iter(
            vec![
                LineItem {
                    len: Rope::from("first line").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..10],
                    generation: 0,
                },
                LineItem {
                    len: Rope::from("this one wraps").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..9, 9..14],
                    generation: 0,
                },
            ],
            &(),
        );

        // Offset 20 is the wrap boundary of the second buffer line: 11 (line start) + 9.
        // It is the last offset of wrap row 1 and the first of wrap row 2 at the same time, so
        // only the affinity can say which row a vertical move should step away from.
        assert_eq!(
            wrapper.offset_to_display_point_with_affinity(20, true),
            WrapDisplayPoint::new(1, 0, 9)
        );
        assert_eq!(
            wrapper.offset_to_display_point_with_affinity(20, false),
            WrapDisplayPoint::new(2, 1, 0)
        );
        assert_eq!(
            wrapper.offset_to_display_point(20),
            wrapper.offset_to_display_point_with_affinity(20, false)
        );

        // An offset that is not on a boundary is unaffected either way.
        for line_end_affinity in [false, true] {
            assert_eq!(
                wrapper.offset_to_display_point_with_affinity(23, line_end_affinity),
                WrapDisplayPoint::new(2, 1, 3)
            );
        }
    }

    #[test]
    fn test_offset_to_display_point() {
        let font = gpui::Font {
            family: "Arial".into(),
            weight: FontWeight::default(),
            style: FontStyle::Normal,
            features: FontFeatures::default(),
            fallbacks: None,
        };

        let mut wrapper = TextWrapper::new(font, px(14.), None);
        wrapper.text = Rope::from(
            "Hello, 世界!\r\nThis is second line.\nThis is third line.\n这里是第 4 行。",
        );
        wrapper.lines = SumTree::from_iter(
            vec![
                // range: 0..15
                LineItem {
                    len: Rope::from("Hello, 世界!\r").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..15],
                    generation: 0,
                },
                // range: 16..36
                LineItem {
                    len: Rope::from("This is second line.\n").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..10, 10..20],
                    generation: 0,
                },
                // range: 37..56
                LineItem {
                    len: Rope::from("This is third line.\n").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..9, 9..15, 15..20],
                    generation: 0,
                },
                // range: 57..79
                LineItem {
                    len: Rope::from("这里是第 4 行。").len(),
                    indent: 0,
                    wrapped_lines: smallvec::smallvec![0..22],
                    generation: 0,
                },
            ],
            &(),
        );

        assert_eq!(
            wrapper.offset_to_display_point(12),
            WrapDisplayPoint::new(0, 0, 12)
        );
        assert_eq!(
            wrapper.offset_to_display_point(15),
            WrapDisplayPoint::new(0, 0, 15)
        );

        assert_eq!(
            wrapper.offset_to_display_point(16),
            WrapDisplayPoint::new(1, 0, 0)
        );
        assert_eq!(
            wrapper.offset_to_display_point(21),
            WrapDisplayPoint::new(1, 0, 5)
        );
        assert_eq!(
            wrapper.offset_to_display_point(27),
            WrapDisplayPoint::new(2, 1, 1)
        );
        assert_eq!(
            wrapper.offset_to_display_point(37),
            WrapDisplayPoint::new(3, 0, 0)
        );
        assert_eq!(
            wrapper.offset_to_display_point(54),
            WrapDisplayPoint::new(5, 2, 2)
        );
        assert_eq!(
            wrapper.offset_to_display_point(59),
            WrapDisplayPoint::new(6, 0, 2)
        );

        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(6, 0, 2)),
            59
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(5, 2, 2)),
            54
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(3, 0, 0)),
            37
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(2, 1, 1)),
            27
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(1, 0, 5)),
            21
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(1, 0, 0)),
            16
        );
        assert_eq!(
            wrapper.display_point_to_offset(WrapDisplayPoint::new(0, 0, 15)),
            15
        );
    }

    #[test]
    fn test_wrapping_indent_same_keeps_indent_reserved() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.0), Some(px(10.)));
        wrapper.params.wrapping_indent = WrappingIndent::Same;
        let text = Rope::from("  abcdefghijklmnopqrstuv");
        let mut fake_wrap_line = |line: &str, _wrap_width: Pixels, _: usize| {
            if line.starts_with(' ') {
                vec![Boundary {
                    ix: 5,
                    next_indent: 2,
                }]
            } else {
                let mut boundaries = vec![];
                let mut i = 8;
                while i < line.len() {
                    boundaries.push(Boundary {
                        ix: i,
                        next_indent: 0,
                    });
                    i += 8;
                }
                boundaries
            }
        };

        wrapper._update(&text, &(0..text.len()), text.len(), &mut fake_wrap_line);

        let line = wrapper.line(0).unwrap();
        assert_eq!(line.indent, 2);
        assert_eq!(line.wrapped_lines.as_slice(), [0..5, 5..24]);
    }

    #[test]
    fn test_wrapping_indent_none_continuation_lines_wrapped_at_full_width() {
        let mut wrapper = TextWrapper::new(test_font(), px(14.0), Some(px(10.)));
        wrapper.params.wrapping_indent = WrappingIndent::None;
        let text = Rope::from("  abcdefghijklmnopqrstuv");
        let mut fake_wrap_line = |line: &str, _wrap_width: Pixels, _: usize| {
            if line.starts_with(' ') {
                vec![Boundary {
                    ix: 5,
                    next_indent: 2,
                }]
            } else {
                let mut boundaries = vec![];
                let mut i = 8;
                while i < line.len() {
                    boundaries.push(Boundary {
                        ix: i,
                        next_indent: 0,
                    });
                    i += 8;
                }
                boundaries
            }
        };

        wrapper._update(&text, &(0..text.len()), text.len(), &mut fake_wrap_line);

        let line = wrapper.line(0).unwrap();
        assert_eq!(line.indent, 0);
        assert_eq!(line.wrapped_lines.as_slice(), [0..5, 5..13, 13..21, 21..24]);
    }

    #[test]
    fn test_wrap_indent_offsets_continuation_lines() {
        let mut line_layout = LineLayout::new();
        line_layout.set_wrapped_lines(smallvec::smallvec![
            ShapedLine::default().with_len(5),
            ShapedLine::default().with_len(10),
        ]);

        line_layout = line_layout.wrap_indent(px(20.0));

        let last_layout = LastLayout {
            visible_range: 0..1,
            visible_buffer_lines: vec![0],
            visible_line_byte_offsets: vec![0],
            visible_top: px(0.),
            visible_range_offset: 0..0,
            lines: Rc::new(vec![]),
            line_height: px(20.0),
            line_number_width: px(0.),
            space_width: px(0.),
            cursor_bounds: None,
            text_align: TextAlign::Left,
            content_width: px(0.),
        };

        assert_eq!(
            line_layout.position_for_index(0, &last_layout, false),
            Some(point(px(0.), px(0.))),
        );

        assert_eq!(
            line_layout.position_for_index(6, &last_layout, false),
            Some(point(px(20.), px(20.))),
        )
    }
}
