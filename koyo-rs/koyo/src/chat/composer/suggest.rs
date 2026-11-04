//! What `@` and `/` complete as they're typed (`docs/chat.md` 5.3): the checkout's files, found as
//! Go to File finds them, and the catalog's commands with Koyo's own. Rows over the footer that
//! leave the keyboard in what's typed and take its keys while they show. A file goes in as `@path`,
//! a command as `/name`, unless it's Koyo's, which is done at once.

use std::ops::Range;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action as _, AnyElement, App, AppContext as _, ClickEvent, Context, Div, FontWeight,
    InteractiveElement as _, IntoElement as _, KeyBinding, ParentElement as _, ScrollStrategy,
    Stateful, StatefulInteractiveElement as _, Styled as _, Task,
    UniformListScrollHandle, Window, actions, div, px,
};
use koyo_project::{Finder, Results};

use self::matching::{CommandRow, Own, Token, TokenKind, commands_matching, naming, token_at};
use super::ChatComposer;
use crate::checkouts;
use crate::files::ROW;
use crate::go_to_file::{self, Row, follow_files};
use crate::list::{self, Activate, PageDown, PageUp, SelectNext, SelectPrevious};
use crate::shell::{self, NewChat};
use crate::theme::Palette;

mod matching;

/// The composer's context while rows show.
pub(super) const SUGGESTING: &str = "ChatSuggesting";
/// Where their keys are heard.
const SUGGESTING_TEXT: &str = "ChatSuggesting > Input";
/// How many files are found: more than anyone goes through, as typing more is quicker.
const MOST_FILES: usize = 50;
/// How many rows show before the rest scroll.
const MOST_SHOWN: usize = 8;

actions!(chat, [CloseSuggestions]);

pub(super) fn bind_keys(cx: &mut App) {
    let suggesting = Some(SUGGESTING_TEXT);
    list::bind_keys(SUGGESTING_TEXT, cx);
    cx.bind_keys([
        KeyBinding::new("enter", Activate, suggesting),
        KeyBinding::new("tab", Activate, suggesting),
        KeyBinding::new("escape", CloseSuggestions, suggesting),
    ]);
}

enum Found {
    Files(Results),
    Commands(Vec<CommandRow>),
}

impl Found {
    fn len(&self) -> usize {
        match self {
            Found::Files(results) => results.found.len(),
            Found::Commands(rows) => rows.len(),
        }
    }
}

/// The checkout's files for as long as a token completes to them, taken up as they change. What
/// the searches kept for erasing back to is let go after, as Go to File does.
struct FileSearch {
    finder: Arc<Finder>,
    _following: Task<()>,
}

impl Drop for FileSearch {
    fn drop(&mut self) {
        self.finder.forget();
    }
}

/// What a token completes to, while it's being typed.
pub(super) struct Suggestions {
    token: Token,
    found: Found,
    at: usize,
    scroll: UniformListScrollHandle,
    searching: Option<Task<()>>,
    /// While it's files that are completed.
    files: Option<FileSearch>,
}

impl Suggestions {
    /// `found` replaces what shows, the row highlighted staying as near as it can.
    fn show(&mut self, found: Found) {
        self.at = self.at.min(found.len().saturating_sub(1));
        self.found = found;
    }

    /// What's being completed is now `token`, so the first of what it finds is the one.
    fn retyped(&mut self, token: Token) {
        self.token = token;
        self.at = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
    }
}

impl ChatComposer {
    /// Whether rows show. A token that completes to nothing doesn't take the keys.
    pub(super) fn suggesting(&self) -> bool {
        self.suggest.as_ref().is_some_and(|open| open.found.len() > 0)
    }

    /// What's typed or where the caret is changed, which may be what's being completed.
    pub(super) fn follow_text(&mut self, cx: &mut Context<Self>) {
        let text = self.text.read(cx);
        let token = (text.selected_range().is_empty())
            .then(|| token_at(&text.value(), text.cursor()))
            .flatten();
        if token.as_ref().map(|token| token.range.start) != self.dismissed {
            self.dismissed = None;
        }
        let token = token.filter(|_| self.dismissed.is_none() && self.picker.is_none());
        let Some(token) = token else {
            if self.suggest.take().is_some() {
                cx.notify();
            }
            return;
        };
        match &mut self.suggest {
            Some(open) if open.token == token => return,
            Some(open) if open.token.kind == token.kind => open.retyped(token),
            _ => {
                let (found, files) = match token.kind {
                    TokenKind::File => (Found::Files(Results::default()), Some(self.follow_files(cx))),
                    TokenKind::Command => (Found::Commands(Vec::new()), None),
                };
                self.suggest = Some(Suggestions {
                    token,
                    found,
                    at: 0,
                    scroll: UniformListScrollHandle::new(),
                    searching: None,
                    files,
                });
            }
        }
        self.search(cx);
        cx.notify();
    }

    /// Takes up the checkout's files now, and as they change for as long as what's returned is held.
    fn follow_files(&self, cx: &mut Context<Self>) -> FileSearch {
        let finder = go_to_file::finder(&self.cwd, cx);
        let root = self.cwd.clone();
        let opened = cx.background_spawn(async move { checkouts::open_git(&root) });
        let following = Arc::clone(&finder);
        let _following = cx.spawn(async move |this, cx| {
            let (git, dirs) = match opened.await {
                Ok(opened) => opened,
                Err(error) => {
                    log::warn!("the files a chat can name: {}", error.message);
                    return;
                }
            };
            let project = cx.update(|cx| checkouts::project(git.root(), dirs, cx));
            drop(git);
            follow_files(this, project, following, cx, |this, changed, _, cx| {
                if changed {
                    this.search(cx);
                }
            })
            .await;
        });
        FileSearch { finder, _following }
    }

    /// Finds what the token completes to.
    pub(super) fn search(&mut self, cx: &mut Context<Self>) {
        let Some(open) = &mut self.suggest else {
            return;
        };
        match &open.files {
            None => {
                let listed = &self.catalog.read(cx).commands;
                let rows = commands_matching(&open.token.query, listed, self.thread.is_some());
                open.show(Found::Commands(rows));
            }
            Some(files) => {
                let (finder, query) = (Arc::clone(&files.finder), open.token.query.clone());
                let found = cx.background_spawn(async move { finder.search(&query, MOST_FILES) });
                open.searching = Some(cx.spawn(async move |this, cx| {
                    if let Some(results) = found.await {
                        this.update(cx, |this, cx| {
                            if let Some(open) = &mut this.suggest {
                                open.show(Found::Files(results));
                                cx.notify();
                            }
                        })
                        .ok();
                    }
                }));
            }
        }
    }

    /// `by` rows up or down, as far as there are any.
    fn move_in_suggestions(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.suggest {
            open.at = list::stepped(Some(open.at), by, open.found.len()).unwrap_or(0);
            open.scroll.scroll_to_item(open.at, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn page_in_suggestions(&mut self, direction: isize, cx: &mut Context<Self>) {
        if let Some(open) = &self.suggest {
            let by = direction * list::page(&open.scroll, ROW);
            self.move_in_suggestions(by, cx);
        }
    }

    /// The pointer moved over row `at`. Moved, as a row that only came under a pointer at rest, the
    /// keys having scrolled it there, mustn't take the highlight.
    fn point_at_suggestion(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(open) = self.suggest.as_mut().filter(|open| open.at != at) {
            open.at = at;
            cx.notify();
        }
    }

    /// The row highlighted takes the token's place, or is done there and then.
    fn pick_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.suggest.take() else {
            return;
        };
        cx.notify();
        let (put, own) = match &open.found {
            Found::Files(results) => {
                let Some(found) = results.found.get(open.at) else {
                    return;
                };
                (naming(&found.path), None)
            }
            Found::Commands(rows) => {
                let Some(row) = rows.get(open.at) else {
                    return;
                };
                match row.own {
                    Some(own) => (String::new(), Some(own)),
                    None => (format!("/{} ", row.name), None),
                }
            }
        };
        self.text.update(cx, |text, cx| {
            text.set_selected_range(open.token.range.clone(), cx);
            text.replace(put, window, cx);
        });
        match own {
            None => {}
            Some(Own::Model) => self.toggle_picker(window, cx),
            Some(Own::Plan) => self.toggle_plan(cx),
            Some(Own::Compact) => self.compact(cx),
            Some(Own::Clear) => window.dispatch_action(NewChat.boxed_clone(), cx),
        }
    }

    /// No rows for this token, however much more of it is typed.
    fn close_suggestions(&mut self, cx: &mut Context<Self>) {
        if let Some(open) = self.suggest.take() {
            self.dismissed = Some(open.token.range.start);
            cx.notify();
        }
    }

    /// `column`, which hears the keys of the rows that show.
    pub(super) fn hears_suggestions(&self, column: Div, cx: &Context<Self>) -> Div {
        column.when(self.suggesting(), |column| {
            column
                .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| {
                    this.move_in_suggestions(-1, cx);
                }))
                .on_action(cx.listener(|this, _: &SelectNext, _, cx| {
                    this.move_in_suggestions(1, cx);
                }))
                .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page_in_suggestions(-1, cx)))
                .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page_in_suggestions(1, cx)))
                .on_action(cx.listener(|this, _: &Activate, window, cx| {
                    this.pick_suggestion(window, cx);
                }))
                .on_action(cx.listener(|this, _: &CloseSuggestions, _, cx| {
                    this.close_suggestions(cx);
                }))
        })
    }

    /// The rows, while there are any.
    pub(super) fn suggestions(
        &self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let open = self.suggest.as_ref().filter(|open| open.found.len() > 0)?;
        let count = open.found.len();
        let rows = cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
            range.filter_map(|at| this.suggestion_row(at, p, cx)).collect()
        });
        let list = list::scrolling("chat-suggestion-rows", count, &open.scroll, rows);
        Some(
            Self::popover("chat-suggestions", p, window, cx)
                .w_full()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_suggestions(cx)))
                .child(
                    div()
                        .h(px(ROW * count.min(MOST_SHOWN) as f32))
                        .flex()
                        .flex_col()
                        .child(list),
                ),
        )
    }

    fn suggestion_row(&self, at: usize, p: Palette, cx: &Context<Self>) -> Option<AnyElement> {
        let open = self.suggest.as_ref()?;
        let id = ("chat-suggestion", at);
        let selected = at == open.at;
        let row = match &open.found {
            Found::Files(results) => Row::of(results, at)?.draw(id, selected, p),
            Found::Commands(rows) => command_row(id, rows.get(at)?, selected, p),
        };
        Some(
            row.on_mouse_move(cx.listener(move |this, _, _, cx| this.point_at_suggestion(at, cx)))
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    this.point_at_suggestion(at, cx);
                    this.pick_suggestion(window, cx);
                }))
                .into_any_element(),
        )
    }
}

fn command_row(
    id: (&'static str, usize),
    row: &CommandRow,
    selected: bool,
    p: Palette,
) -> Stateful<Div> {
    shell::row(id, selected, p)
        .w_full()
        .h(px(ROW))
        .gap_1p5()
        .rounded(px(5.))
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("/{}", row.name)),
        )
        .children(row.hint.clone().map(|hint| div().flex_shrink_0().text_color(p.muted).child(hint)))
        .child(shell::ellipsized(row.description.clone()).flex_1().text_color(p.muted))
}
