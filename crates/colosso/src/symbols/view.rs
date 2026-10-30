//! The palette's state: the query, the symbols it's narrowed to and the row picked. A file's
//! symbols come once and are narrowed here as the query's typed; the checkout's are asked
//! of its servers for each query, a moment after the typing pauses, and shown as they say.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::files::{Place, ROW, relative};
use crate::language::{self, Symbol};
use crate::list::{page, stepped};
use crate::modal;
use crate::shell::Shell;

/// How long the typing pauses before the checkout's servers are asked.
const PAUSE: Duration = Duration::from_millis(100);

/// A row as the tests see it: the symbol's name, what it's in, the characters of the name
/// that matched, and its file.
#[cfg(test)]
pub type Row = (String, Option<String>, String, Option<String>);

/// Where the symbols come from.
pub enum Source {
    /// The file shown: its symbols, listed once.
    Document(Task<Vec<Symbol>>),
    /// The checkout at the root: its servers, asked for each query.
    Workspace(PathBuf),
}

/// Opens the palette over `window`, on `source`'s symbols; the one picked is shown by
/// `shell`.
pub fn open(source: Source, shell: WeakEntity<Shell>, window: &mut Window, cx: &mut App) {
    let behind = window.window_handle();
    modal::open_palette(window, cx, |window, cx| {
        Symbols::new(source, shell, behind, window, cx)
    });
}

pub struct Symbols {
    pub(super) input: Entity<InputState>,
    /// The checkout's root, when its servers are asked for each query; else the file's
    /// symbols are all known.
    pub(super) root: Option<PathBuf>,
    /// Whether the checkout has a server to ask; one starts with the first of its files
    /// shown.
    pub(super) served: bool,
    /// What's typed, trimmed.
    pub(super) query: String,
    /// The symbols known, as they came.
    pub(super) known: Vec<Symbol>,
    /// Those shown, by index into `known`, with the bytes of the name the query matched.
    pub(super) shown: Vec<(usize, Vec<Range<usize>>)>,
    pub(super) selected: usize,
    pub(super) scroll: UniformListScrollHandle,
    /// The symbols being listed, while they are.
    pub(super) asking: Option<Task<()>>,
    matcher: Matcher,
    chars: Vec<char>,
    shell: WeakEntity<Shell>,
    /// The window the palette sits over, where files open.
    behind: AnyWindowHandle,
    _input: Subscription,
}

impl Symbols {
    fn new(
        source: Source,
        shell: WeakEntity<Shell>,
        behind: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (placeholder, root, answer) = match source {
            Source::Document(symbols) => ("Go to symbol in file", None, Some(symbols)),
            Source::Workspace(root) => ("Go to symbol in checkout", Some(root), None),
        };
        let served = (root.as_deref()).is_none_or(|root| language::serves(root, cx));
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        input.focus_handle(cx).focus(window, cx);
        let _input = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            InputEvent::Change => this.typed(cx),
            InputEvent::PressEnter { .. } => this.open(this.selected, window, cx),
            _ => {}
        });
        let asking = answer.map(|answer| {
            cx.spawn(async move |this, cx| {
                let symbols = answer.await;
                this.update(cx, |this, cx| this.take(symbols, cx)).ok();
            })
        });
        Symbols {
            input,
            root,
            served,
            query: String::new(),
            known: Vec::new(),
            shown: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            asking,
            matcher: Matcher::new(Config::DEFAULT),
            chars: Vec::new(),
            shell,
            behind,
            _input,
        }
    }

    /// The query changed: a file's symbols are narrowed now; the checkout's servers are
    /// asked once the typing pauses, and for nothing when nothing's typed.
    fn typed(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).value().trim().to_owned();
        if query == self.query {
            return;
        }
        self.query = query;
        let Some(root) = self.root.clone() else {
            self.narrow(cx);
            return;
        };
        if self.query.is_empty() {
            self.asking = None;
            self.take(Vec::new(), cx);
            return;
        }
        let query = self.query.clone();
        self.asking = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            let asked = this.update(cx, |_, cx| language::workspace_symbols(&root, query, cx));
            let Ok(asked) = asked else {
                return;
            };
            let symbols = asked.await;
            this.update(cx, |this, cx| this.take(symbols, cx)).ok();
        }));
        cx.notify();
    }

    /// Takes `symbols` as the ones known, and shows those the query matches.
    fn take(&mut self, symbols: Vec<Symbol>, cx: &mut Context<Self>) {
        self.known = symbols;
        self.asking = None;
        self.narrow(cx);
    }

    /// Shows the symbols whose name the query matches, best first; all of them, in the
    /// order given, for none. The checkout's servers match their own way: what they gave
    /// is shown even when the name isn't matched here, just without the bytes marked.
    fn narrow(&mut self, cx: &mut Context<Self>) {
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let served = self.root.is_some();
        let mut indices = Vec::new();
        let mut shown: Vec<(u32, usize, Vec<Range<usize>>)> = (self.known.iter().enumerate())
            .filter_map(|(index, symbol)| {
                indices.clear();
                let name = Utf32Str::new(&symbol.name, &mut self.chars);
                match pattern.indices(name, &mut self.matcher, &mut indices) {
                    Some(score) => {
                        let matched = colosso_project::matched_bytes(&symbol.name, &mut indices);
                        Some((score, index, matched))
                    }
                    None if served => Some((0, index, Vec::new())),
                    None => None,
                }
            })
            .collect();
        shown.sort_by_key(|row| std::cmp::Reverse(row.0));
        self.shown = (shown.into_iter()).map(|(_, index, matched)| (index, matched)).collect();
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    /// The symbol at row `index`, with the bytes of its name that matched.
    pub(super) fn row_symbol(&self, index: usize) -> Option<(&Symbol, &[Range<usize>])> {
        let (at, matched) = self.shown.get(index)?;
        Some((&self.known[*at], matched))
    }

    /// `symbol`'s file as its row names it, from the checkout whose servers were asked;
    /// nothing for the file shown, whose symbols are all in it.
    pub(super) fn file_of(&self, symbol: &Symbol) -> Option<SharedString> {
        (self.root.as_deref()).map(|root| relative(root, &symbol.path))
    }

    /// Moves the pick `by` rows, stopping at either end.
    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        self.selected = stepped(Some(self.selected), by, self.shown.len()).unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub(super) fn page(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.step(direction * page(&self.scroll, ROW), cx);
    }

    /// Shows the symbol at row `index` in the window behind, its name selected, closing
    /// the palette.
    pub(super) fn open(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((symbol, _)) = self.row_symbol(index) else {
            return;
        };
        let place = Place {
            path: symbol.path.to_path_buf(),
            at: symbol.at.clone(),
        };
        modal::dismiss(window, cx);
        Shell::view_behind(&self.shell, self.behind, place, cx);
    }

    /// The rows as shown.
    #[cfg(test)]
    pub fn rows(&self) -> Vec<Row> {
        (self.shown.iter())
            .map(|(at, matched)| {
                let symbol = &self.known[*at];
                let container = symbol.container.as_ref().map(ToString::to_string);
                let marked = (matched.iter()).map(|range| &symbol.name[range.clone()]).collect();
                let file = self.file_of(symbol).map(|file| file.to_string());
                (symbol.name.to_string(), container, marked, file)
            })
            .collect()
    }
}
