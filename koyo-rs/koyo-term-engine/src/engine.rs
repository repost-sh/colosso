//! The engine: one libghostty-vt terminal with the render state that reads frames off it.

use std::collections::HashMap;
use std::sync::Arc;

use ghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use ghostty_vt::render::{
    CellIterator, CursorVisualStyle, Dirty as RenderDirty, RenderState, RowId, RowIterator,
};
use ghostty_vt::screen::{CellWide, RowSemanticPrompt};
use ghostty_vt::style::{RgbColor, StyleColor, Underline as StyleUnderline};
use ghostty_vt::terminal::{ColorScheme, Event, Mode, ProgressState, ScrollViewport};
use ghostty_vt::{Terminal, TerminalOptions};

use crate::Result;
use crate::frame::{
    Attrs, Cell, Cursor, CursorFlags, CursorShape, Dirty, Frame, Grapheme, Rgb, Row, RowKind,
    Scrollbar, Underline, Width,
};

mod links;

/// The grid's size in cells and the cell's size in pixels, which size reports and images use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

impl Size {
    pub const fn cells(cols: u16, rows: u16) -> Size {
        Size {
            cols,
            rows,
            cell_width_px: 0,
            cell_height_px: 0,
        }
    }
}

/// A side effect of the bytes fed in, for whoever owns the PTY or the window to act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// A reply the program asked for (a query's answer, a mode report): write it to the PTY.
    WriteBack(Vec<u8>),
    Bell,
    Title(String),
    /// The working directory as the shell reported it, a `file://` URI for OSC 7.
    Pwd(String),
    /// The program wrote the clipboard: the MIME representations of one value.
    Clipboard(Vec<(String, Vec<u8>)>),
    Notification {
        title: String,
        body: String,
    },
    Progress(Progress),
    /// Synchronized output began (`true`) or ended (`false`): don't paint while it's on.
    RenderHold(bool),
}

/// A progress report, OSC 9;4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Remove,
    Set(u8),
    Error(Option<u8>),
    Indeterminate,
    Paused(Option<u8>),
}

/// How to scroll the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroll {
    Top,
    Bottom,
    /// Rows, negative being up into the scrollback.
    Rows(i32),
}

/// What text to format the screen as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFormat {
    Plain,
    /// With the escape sequences that restore colours and styles.
    Vt,
    Html,
}

/// One terminal: Ghostty's parser and grid, plus the render state frames are read from.
pub struct Engine {
    pub(crate) term: Terminal,
    pub(crate) encoders: crate::input::Encoders,
    render: RenderState,
    rows: RowIterator,
    cells: CellIterator,
    /// Rows of the last frame by identity, reused while they stay clean.
    cache: HashMap<RowId, Arc<Row>>,
    sequence: u64,
    grapheme: Vec<char>,
    /// Where a link's URI is read into, grown to the longest seen.
    uri: Vec<u8>,
    /// Whether the host's theme is dark, once it's said: what a program asking is told.
    dark: Option<bool>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// A terminal of `size` keeping about `scrollback_bytes` of history.
    pub fn new(size: Size, scrollback_bytes: usize) -> Result<Engine> {
        let term = Terminal::new(TerminalOptions {
            cols: size.cols.max(1),
            rows: size.rows.max(1),
            max_scrollback: scrollback_bytes,
        })?;
        let mut engine = Engine::around(term)?;
        if size.cell_width_px > 0 && size.cell_height_px > 0 {
            engine.resize(size)?;
        }
        Ok(engine)
    }

    /// An engine restored from a snapshot another engine encoded (the same build of it).
    pub fn restore(snapshot: &[u8]) -> Result<Engine> {
        let term = ghostty_vt::snapshot::Decoder::from_slice(snapshot).decode()?;
        Engine::around(term)
    }

    fn around(term: Terminal) -> Result<Engine> {
        Ok(Engine {
            term,
            encoders: crate::input::Encoders::new()?,
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            cache: HashMap::new(),
            sequence: 0,
            grapheme: Vec::with_capacity(8),
            uri: Vec::new(),
            dark: None,
        })
    }

    /// Feeds the program's output. Never fails: bad input is dropped, not propagated.
    pub fn write(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    /// The effects the bytes fed since the last call had, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        self.term.events().map(effect).collect()
    }

    pub fn resize(&mut self, size: Size) -> Result<()> {
        self.term.resize(
            size.cols.max(1),
            size.rows.max(1),
            size.cell_width_px,
            size.cell_height_px,
        )
    }

    pub fn cols(&self) -> u16 {
        self.term.cols().unwrap_or(0)
    }

    pub fn rows(&self) -> u16 {
        self.term.rows().unwrap_or(0)
    }

    pub fn title(&self) -> String {
        self.term.title().unwrap_or_default().to_owned()
    }

    /// The working directory the shell reported, a `file://` URI, or empty.
    pub fn pwd(&self) -> String {
        self.term.pwd().unwrap_or_default().to_owned()
    }

    pub fn is_alt_screen(&self) -> bool {
        matches!(
            self.term.active_screen(),
            Ok(ghostty_vt::screen::Screen::Alternate)
        )
    }

    pub fn is_mouse_tracking(&self) -> bool {
        self.term.is_mouse_tracking().unwrap_or(false)
    }

    /// Whether the shell's cursor sits at a prompt, per its OSC 133 marks.
    pub fn is_cursor_at_prompt(&self) -> bool {
        self.term.is_cursor_at_prompt().unwrap_or(false)
    }

    pub fn is_bracketed_paste(&self) -> bool {
        self.term.mode(Mode::BRACKETED_PASTE).unwrap_or(false)
    }

    pub fn scrollbar(&self) -> Scrollbar {
        self.term
            .scrollbar()
            .map(|s| Scrollbar {
                total: s.total,
                offset: s.offset,
                len: s.len,
            })
            .unwrap_or_default()
    }

    pub fn scroll(&mut self, by: Scroll) {
        self.term.scroll_viewport(match by {
            Scroll::Top => ScrollViewport::Top,
            Scroll::Bottom => ScrollViewport::Bottom,
            Scroll::Rows(delta) => ScrollViewport::Delta(delta as isize),
        });
    }

    /// The default colours, from the theme; the program's OSC overrides still win.
    pub fn set_default_colors(&mut self, foreground: Rgb, background: Rgb) -> Result<()> {
        self.term
            .set_default_fg_color(Some(rgb_color(foreground)))?;
        self.term
            .set_default_bg_color(Some(rgb_color(background)))?;
        Ok(())
    }

    /// Whether the host's theme is dark, as a program asking (CSI ? 996 n) is told. When it
    /// changes for a program that asked to hear of changes (mode 2031), the report to write
    /// to it.
    pub fn set_color_scheme(&mut self, dark: bool) -> Result<Option<Vec<u8>>> {
        let changed = self.dark.replace(dark).is_some_and(|was| was != dark);
        let scheme = if dark {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        };
        self.term.set_color_scheme(Some(scheme));
        if !changed || !self.term.mode(Mode::COLOR_SCHEME_REPORT)? {
            return Ok(None);
        }
        let mut report = [0; 16];
        let len = scheme.encode_report(&mut report)?;
        Ok(Some(report[..len].to_vec()))
    }

    /// The whole state, to restore with [`Engine::restore`] or to send to another client.
    pub fn snapshot(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        ghostty_vt::snapshot::encode_to(&self.term, &mut bytes)?;
        Ok(bytes)
    }

    /// The screen as text: the active screen's rows, as the formatter lays them out.
    pub fn screen_text(&self, format: TextFormat) -> Result<String> {
        let options = FormatterOptions::new()
            .with_format(match format {
                TextFormat::Plain => Format::Plain,
                TextFormat::Vt => Format::Vt,
                TextFormat::Html => Format::Html,
            })
            .with_trim(true);
        let mut formatter = Formatter::new(&self.term, options)?;
        // Written to a buffer rather than allocated by the formatter, since an empty screen
        // formats to nothing, which the allocating call reports as a failure.
        let mut bytes = Vec::new();
        formatter.format_to(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Reads a frame off the terminal and marks the render state clean.
    pub fn frame(&mut self) -> Result<Frame> {
        self.sequence += 1;
        let snapshot = self.render.update(&self.term)?;
        let dirty = match snapshot.dirty()? {
            RenderDirty::Clean => Dirty::Clean,
            RenderDirty::Partial => Dirty::Partial,
            RenderDirty::Full => Dirty::Full,
        };
        let cols = snapshot.cols()?;
        let row_count = snapshot.rows()? as usize;
        let colors = snapshot.colors()?;
        let cursor = snapshot.cursor()?;

        let mut rows = Vec::with_capacity(row_count);
        let mut dirty_rows = Vec::with_capacity(row_count);
        let mut cache = HashMap::with_capacity(row_count);
        {
            let mut iteration = self.rows.update(&snapshot)?;
            while let Some(row) = iteration.next() {
                let id = row.id()?;
                let row_dirty = dirty == Dirty::Full || row.dirty()?;
                let kept = (!row_dirty).then(|| self.cache.get(&id).cloned()).flatten();
                let shared = if let Some(shared) = kept {
                    shared
                } else {
                    let raw = row.raw_row()?;
                    let mut cells = Vec::with_capacity(cols as usize);
                    // The columns of cells that are links, in a row that may have any.
                    let linked = raw.has_hyperlink().unwrap_or(false);
                    let mut linked_cols = Vec::new();
                    let mut cell_iteration = self.cells.update(row)?;
                    while let Some(cell) = cell_iteration.next() {
                        if linked && cell.raw_cell()?.has_hyperlink().unwrap_or(false) {
                            linked_cols.push(cells.len() as u16);
                        }
                        cells.push(read_cell(cell, &mut self.grapheme)?);
                    }
                    let links = if linked_cols.is_empty() {
                        Vec::new()
                    } else {
                        let y = row.viewport_y()?;
                        links::read(&self.term, y, &linked_cols, &cells, &mut self.uri)
                    };
                    Arc::new(Row {
                        cells,
                        wrapped: raw.is_wrapped().unwrap_or(false),
                        kind: match raw.semantic_prompt().unwrap_or(RowSemanticPrompt::None) {
                            RowSemanticPrompt::None => RowKind::Output,
                            RowSemanticPrompt::Prompt => RowKind::Prompt,
                            RowSemanticPrompt::Continuation => RowKind::PromptContinuation,
                        },
                        links,
                    })
                };
                cache.insert(id, Arc::clone(&shared));
                rows.push(shared);
                dirty_rows.push(row_dirty);
            }
        }
        snapshot.clean()?;
        self.cache = cache;

        Ok(Frame {
            sequence: self.sequence,
            cols,
            rows,
            dirty_rows,
            dirty,
            cursor: cursor.viewport.map(|at| {
                let mut flags = CursorFlags::NONE;
                flags.set(CursorFlags::VISIBLE, cursor.visible);
                flags.set(CursorFlags::BLINKING, cursor.blinking);
                flags.set(CursorFlags::AT_WIDE_TAIL, at.at_wide_tail);
                flags.set(CursorFlags::PASSWORD_INPUT, cursor.password_input);
                Cursor {
                    col: at.x,
                    row: at.y,
                    shape: match cursor.visual_style {
                        CursorVisualStyle::Bar => CursorShape::Bar,
                        CursorVisualStyle::Underline => CursorShape::Underline,
                        CursorVisualStyle::BlockHollow => CursorShape::HollowBlock,
                        _ => CursorShape::Block,
                    },
                    flags,
                }
            }),
            foreground: rgb(colors.foreground),
            background: rgb(colors.background),
            background_overridden: self.term.bg_color()? != self.term.default_bg_color()?,
            cursor_color: colors.cursor.map(rgb),
            scrollbar: self.scrollbar(),
            alt_screen: self.is_alt_screen(),
            mouse_tracking: self.is_mouse_tracking(),
            title: self.title(),
            pwd: self.pwd(),
        })
    }
}

fn read_cell(
    cell: &ghostty_vt::render::CellIteration<'_>,
    grapheme: &mut Vec<char>,
) -> Result<Cell> {
    let raw = cell.raw_cell()?;
    let width = match raw.wide().unwrap_or(CellWide::Narrow) {
        CellWide::Narrow => Width::Narrow,
        CellWide::Wide => Width::Wide,
        CellWide::SpacerTail => Width::SpacerTail,
        CellWide::SpacerHead => Width::SpacerHead,
    };
    let text = if raw.has_text().unwrap_or(false) {
        let len = cell.graphemes_len()?;
        grapheme.clear();
        grapheme.resize(len, '\0');
        cell.graphemes_buf(grapheme)?;
        match grapheme.as_slice() {
            [] | ['\0' | ' '] => Grapheme::Empty,
            [single] => Grapheme::Char(*single),
            cluster => Grapheme::Cluster(cluster.iter().collect::<String>().into_boxed_str()),
        }
    } else {
        Grapheme::Empty
    };
    let mut out = Cell {
        grapheme: text,
        fg: cell.fg_color()?.map(rgb),
        bg: cell.bg_color()?.map(rgb),
        underline_color: None,
        underline: Underline::None,
        width,
        attrs: Attrs::NONE,
    };
    if cell.has_styling()? {
        let style = cell.style()?;
        out.underline = match style.underline {
            StyleUnderline::Single => Underline::Single,
            StyleUnderline::Double => Underline::Double,
            StyleUnderline::Curly => Underline::Curly,
            StyleUnderline::Dotted => Underline::Dotted,
            StyleUnderline::Dashed => Underline::Dashed,
            _ => Underline::None,
        };
        out.underline_color = match style.underline_color {
            StyleColor::Rgb(color) => Some(rgb(color)),
            _ => None,
        };
        out.attrs.set(Attrs::BOLD, style.bold);
        out.attrs.set(Attrs::ITALIC, style.italic);
        out.attrs.set(Attrs::FAINT, style.faint);
        out.attrs.set(Attrs::BLINK, style.blink);
        out.attrs.set(Attrs::INVERSE, style.inverse);
        out.attrs.set(Attrs::INVISIBLE, style.invisible);
        out.attrs.set(Attrs::STRIKETHROUGH, style.strikethrough);
        out.attrs.set(Attrs::OVERLINE, style.overline);
    }
    out.attrs.set(Attrs::SELECTED, cell.is_selected()?);
    Ok(out)
}

fn effect(event: Event) -> Effect {
    match event {
        Event::PtyWrite(bytes) => Effect::WriteBack(bytes),
        Event::Bell => Effect::Bell,
        Event::TitleChanged(title) => Effect::Title(title),
        Event::PwdChanged(pwd) => Effect::Pwd(pwd),
        Event::ClipboardWrite(write) => Effect::Clipboard(
            write
                .contents
                .into_iter()
                .map(|c| (c.mime, c.data))
                .collect(),
        ),
        Event::DesktopNotification { title, body } => Effect::Notification { title, body },
        Event::ProgressReport { state, progress } => Effect::Progress(match state {
            ProgressState::Remove => Progress::Remove,
            ProgressState::Set => Progress::Set(progress.unwrap_or(0)),
            ProgressState::Error => Progress::Error(progress),
            ProgressState::Indeterminate => Progress::Indeterminate,
            ProgressState::Pause => Progress::Paused(progress),
        }),
        Event::RenderHold(on) => Effect::RenderHold(on),
        // The binding may grow events this build doesn't know; none is lost that matters here.
        #[expect(unreachable_patterns, reason = "the binding's enum may gain variants")]
        _ => Effect::RenderHold(false),
    }
}

fn rgb(color: RgbColor) -> Rgb {
    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

fn rgb_color(color: Rgb) -> RgbColor {
    RgbColor {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

#[cfg(test)]
mod tests;
