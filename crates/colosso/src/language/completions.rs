//! What could complete the word being typed: asked of the server once per word, as the
//! list it gives holds everything for the word, and filtered and ranked here as the word
//! goes on, with Helix's matcher; its rows' edits moved past what was typed since. The row
//! selected is filled in by the server, for its documentation and the edits that come with
//! it, and kept filled in.

use std::cell::RefCell;
use std::rc::Rc;

use colosso_lsp::lsp_types::request::{Completion, ResolveCompletionItem};
use colosso_lsp::lsp_types::{
    self, CompletionContext, CompletionItem, CompletionParams, CompletionResponse,
    CompletionTextEdit, CompletionTriggerKind, PartialResultParams, Position,
    WorkDoneProgressParams,
};
use gpui_kit::component::input::{
    CompletionHighlighter, CompletionProvider, Rope, RopeExt as _, is_identifier_char,
};
use gpui_kit::{App, AppContext as _, Task, Window};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use super::providers::{Asked, Hooks};

/// The list the server last gave, and what ranks it.
pub(super) struct Completions {
    matcher: Matcher,
    chars: Vec<char>,
    list: Option<List>,
}

/// The list the server gave for a word, as the server gave it, in the text as it was then.
struct List {
    /// Where the word begins, and the caret it was asked at, in the editor's positions.
    start: usize,
    caret: Position,
    /// The word as it was when the list was asked for: the list holds everything for any
    /// longer word, unless the server said it's incomplete.
    asked_for: String,
    incomplete: bool,
    items: Vec<CompletionItem>,
    /// Which rows the server has filled in.
    filled: Vec<bool>,
    /// The word as it was when the list was last served.
    served_for: String,
}

impl Default for Completions {
    fn default() -> Self {
        Completions {
            matcher: Matcher::new(Config::DEFAULT),
            chars: Vec::new(),
            list: None,
        }
    }
}

impl Completions {
    /// The list for the word `query` begun at `start`, ranked and brought up to the word,
    /// if the server's list holds everything for it.
    fn ranked(&mut self, start: usize, query: &str) -> Option<Vec<CompletionItem>> {
        let Completions { matcher, chars, list } = self;
        let list = list.as_mut()?;
        if list.start != start || list.incomplete || !query.starts_with(&list.asked_for) {
            return None;
        }
        query.clone_into(&mut list.served_for);
        let ranked = rank(&list.items, query, matcher, chars);
        Some(ranked.into_iter().map(|item| list.typed_on(item)).collect())
    }

    /// Keeps `items`, the server's list for the word `asked_for` begun at `start`, asked
    /// at `caret`.
    fn keep(&mut self, start: usize, caret: Position, asked_for: String, incomplete: bool, items: Vec<CompletionItem>) {
        self.list = Some(List {
            start,
            caret,
            served_for: asked_for.clone(),
            asked_for,
            incomplete,
            filled: vec![false; items.len()],
            items,
        });
    }

    /// The row `item` is, if it's filled in already, brought up to the word.
    fn filled(&self, item: &CompletionItem) -> Option<CompletionItem> {
        let list = self.list.as_ref()?;
        let ix = list.row(item)?;
        list.filled[ix].then(|| list.typed_on(list.items[ix].clone()))
    }

    /// Keeps `filled`, a row the server filled in, in place of the row it fills in, and
    /// hands it back brought up to the word.
    fn fill(&mut self, filled: CompletionItem) -> CompletionItem {
        let Some(list) = self.list.as_mut() else {
            return filled;
        };
        if let Some(ix) = list.row(&filled) {
            list.items[ix] = filled.clone();
            list.filled[ix] = true;
        }
        list.typed_on(filled)
    }

    /// Marks the row `item` is as filled in as it gets.
    fn mark_filled(&mut self, item: &CompletionItem) {
        if let Some(list) = self.list.as_mut()
            && let Some(ix) = list.row(item)
        {
            list.filled[ix] = true;
        }
    }

    /// Whether the list is for the word begun at `start`, and the server said it's
    /// incomplete.
    fn incomplete_at(&self, start: usize) -> bool {
        (self.list.as_ref()).is_some_and(|list| list.incomplete && list.start == start)
    }
}

impl List {
    fn row(&self, item: &CompletionItem) -> Option<usize> {
        self.items.iter().position(|row| same_row(row, item))
    }

    /// `item`'s edits moved past what was typed at the caret since the list was asked
    /// for: they are in the text as it was then.
    fn typed_on(&self, mut item: CompletionItem) -> CompletionItem {
        let typed = self.served_for[self.asked_for.len()..].chars().count() as u32;
        if typed == 0 {
            return item;
        }
        let moved = |mut position: Position| {
            if position.line == self.caret.line && position.character >= self.caret.character {
                position.character += typed;
            }
            position
        };
        let moved_range = |range: &mut lsp_types::Range| {
            range.start = moved(range.start);
            range.end = moved(range.end);
        };
        match &mut item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => moved_range(&mut edit.range),
            Some(CompletionTextEdit::InsertAndReplace(edit)) => {
                moved_range(&mut edit.insert);
                moved_range(&mut edit.replace);
            }
            None => {}
        }
        for edit in item.additional_text_edits.iter_mut().flatten() {
            moved_range(&mut edit.range);
        }
        item
    }
}

/// Whether two rows are the same row of the server's list, filled in or not.
fn same_row(a: &CompletionItem, b: &CompletionItem) -> bool {
    a.label == b.label && a.kind == b.kind && a.sort_text == b.sort_text
}

/// `items` that match `query`, best first; all of them in the server's order for none.
fn rank(
    items: &[CompletionItem],
    query: &str,
    matcher: &mut Matcher,
    chars: &mut Vec<char>,
) -> Vec<CompletionItem> {
    // Ties broken by the server's say: preselected first, then by its sort text.
    fn key(item: &CompletionItem) -> (bool, &str) {
        (
            !item.preselect.unwrap_or(false),
            item.sort_text.as_deref().unwrap_or(&item.label),
        )
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut ranked: Vec<(u32, &CompletionItem)> = (items.iter())
        .filter_map(|item| {
            let text = item.filter_text.as_deref().unwrap_or(&item.label);
            let score = pattern.score(Utf32Str::new(text, chars), matcher)?;
            Some((score, item))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| key(a.1).cmp(&key(b.1))));
    ranked.into_iter().map(|(_, item)| item.clone()).collect()
}

/// Paints the bytes of a row's label that the word typed matches, as the matcher sees it.
pub(super) fn highlighter() -> Rc<CompletionHighlighter> {
    struct Highlighting {
        matcher: Matcher,
        chars: Vec<char>,
        indices: Vec<u32>,
        /// The word last painted for, parsed.
        pattern: Option<(String, Pattern)>,
    }
    let highlighting = RefCell::new(Highlighting {
        matcher: Matcher::new(Config::DEFAULT),
        chars: Vec::new(),
        indices: Vec::new(),
        pattern: None,
    });
    Rc::new(move |query: &str, item: &CompletionItem| {
        if query.is_empty() {
            return Vec::new();
        }
        let Highlighting { matcher, chars, indices, pattern } = &mut *highlighting.borrow_mut();
        let (_, pattern) = match pattern {
            Some(parsed) if parsed.0 == query => parsed,
            slot => slot.insert((
                query.to_owned(),
                Pattern::parse(query, CaseMatching::Smart, Normalization::Smart),
            )),
        };
        indices.clear();
        pattern.indices(Utf32Str::new(&item.label, chars), matcher, indices);
        colosso_project::matched_bytes(&item.label, indices)
    })
}

impl Asked {
    /// A row's edits, from the server's encoding to the editor's positions.
    fn row_for_editor(&self, mut item: CompletionItem) -> CompletionItem {
        item.text_edit = item.text_edit.map(|edit| match edit {
            CompletionTextEdit::Edit(mut edit) => {
                edit.range = self.for_editor(edit.range);
                CompletionTextEdit::Edit(edit)
            }
            CompletionTextEdit::InsertAndReplace(mut edit) => {
                edit.insert = self.for_editor(edit.insert);
                edit.replace = self.for_editor(edit.replace);
                CompletionTextEdit::InsertAndReplace(edit)
            }
        });
        for edit in item.additional_text_edits.iter_mut().flatten() {
            edit.range = self.for_editor(edit.range);
        }
        item
    }
}

impl CompletionProvider for Hooks {
    /// A character of a word, or one the server asked to hear of, a `.` say.
    fn is_completion_trigger(&self, _: usize, new_text: &str, cx: &mut App) -> bool {
        let mut typed = new_text.chars();
        let (Some(ch), None) = (typed.next(), typed.next()) else {
            return false;
        };
        is_identifier_char(ch) || self.is_trigger_character(new_text, cx)
    }

    /// Asked by the editor once it's done with the edit, which the server has heard of.
    fn completions(
        &self,
        rope: &Rope,
        offset: usize,
        context: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<CompletionResponse>> {
        let start = rope.identifier_start(offset);
        let query = rope.slice(start..offset).to_string();
        if let Some(items) = self.completions.borrow_mut().ranked(start, &query) {
            return Task::ready(Ok(CompletionResponse::Array(items)));
        }
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(Ok(CompletionResponse::Array(Vec::new())));
        };
        let typed = context.trigger_character.filter(|typed| {
            context.trigger_kind == CompletionTriggerKind::TRIGGER_CHARACTER
                && self.is_trigger_character(typed, cx)
        });
        let incomplete = self.completions.borrow().incomplete_at(start);
        let caret = rope.offset_to_position(offset);
        let completions = self.completions.clone();
        cx.spawn(async move |cx| {
            let asked_for = query.clone();
            let (items, ranked, incomplete) = cx
                .background_spawn(async move {
                    let context = CompletionContext {
                        trigger_kind: match (&typed, incomplete) {
                            (_, true) => CompletionTriggerKind::TRIGGER_FOR_INCOMPLETE_COMPLETIONS,
                            (Some(_), false) => CompletionTriggerKind::TRIGGER_CHARACTER,
                            (None, false) => CompletionTriggerKind::INVOKED,
                        },
                        trigger_character: typed,
                    };
                    let params = CompletionParams {
                        text_document_position: asked.at(offset),
                        work_done_progress_params: WorkDoneProgressParams::default(),
                        partial_result_params: PartialResultParams::default(),
                        context: Some(context),
                    };
                    let (items, incomplete) = match asked.ask::<Completion>(&params).await.flatten() {
                        Some(CompletionResponse::Array(items)) => (items, false),
                        Some(CompletionResponse::List(list)) => (list.items, list.is_incomplete),
                        None => (Vec::new(), false),
                    };
                    let items = (items.into_iter())
                        .map(|item| asked.row_for_editor(item))
                        .collect::<Vec<_>>();
                    let ranked = rank(&items, &query, &mut Matcher::new(Config::DEFAULT), &mut Vec::new());
                    (items, ranked, incomplete)
                })
                .await;
            (completions.borrow_mut()).keep(start, caret, asked_for, incomplete, items);
            Ok(CompletionResponse::Array(ranked))
        })
    }

    /// The row filled in by the server, once; a row filled in already comes from the list.
    fn resolve(&self, item: CompletionItem, cx: &mut App) -> Task<gpui_kit::Result<CompletionItem>> {
        if let Some(filled) = self.completions.borrow().filled(&item) {
            return Task::ready(Ok(filled));
        }
        let resolves = (self.capabilities(cx)).is_some_and(|capabilities| {
            (capabilities.completion_provider.as_ref())
                .is_some_and(|options| options.resolve_provider == Some(true))
        });
        let Some(asked) = resolves.then(|| self.as_shown(cx)).flatten() else {
            // Nothing more will come for it: as filled in as it gets.
            self.completions.borrow_mut().mark_filled(&item);
            return Task::ready(Ok(item));
        };
        let completions = self.completions.clone();
        cx.spawn(async move |cx| {
            let filled = cx
                .background_spawn(async move {
                    let answer = asked.ask::<ResolveCompletionItem>(&item).await;
                    answer.map_or(item, |filled| asked.row_for_editor(filled))
                })
                .await;
            Ok(completions.borrow_mut().fill(filled))
        })
    }
}

impl Hooks {
    /// Whether `text` is a character the server asked to hear of as it's typed.
    fn is_trigger_character(&self, text: &str, cx: &App) -> bool {
        (self.capabilities(cx)).is_some_and(|capabilities| {
            (capabilities.completion_provider.as_ref())
                .and_then(|options| options.trigger_characters.as_deref())
                .is_some_and(|triggers| triggers.iter().any(|trigger| trigger == text))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, sort_text: Option<&str>, preselect: bool) -> CompletionItem {
        CompletionItem {
            label: label.to_owned(),
            sort_text: sort_text.map(str::to_owned),
            preselect: Some(preselect),
            ..CompletionItem::default()
        }
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    fn ranked(items: &[CompletionItem], query: &str) -> Vec<CompletionItem> {
        rank(items, query, &mut Matcher::new(Config::DEFAULT), &mut Vec::new())
    }

    #[test]
    fn rows_are_ranked_by_match_then_preselection_then_sort_text() {
        let items = [
            item("other_thing", None, false),
            item("grab", None, false),
            item("greet", None, true),
            item("GREP_ALL", Some("0000"), false),
        ];
        assert_eq!(labels(&ranked(&items, "gr")), ["greet", "GREP_ALL", "grab"]);
        assert_eq!(labels(&ranked(&items, "grb")), ["grab"]);
        assert_eq!(labels(&ranked(&items, "zzz")), Vec::<&str>::new());
        // Nothing typed: the server's order, the preselected row first.
        assert_eq!(
            labels(&ranked(&items, "")),
            ["greet", "GREP_ALL", "grab", "other_thing"]
        );
    }

    #[test]
    fn a_row_is_matched_by_its_filter_text_and_painted_on_its_label() {
        let mut full = item("greet(…)", None, false);
        full.filter_text = Some("greet".to_owned());
        assert_eq!(labels(&ranked(&[full.clone()], "gre")), ["greet(…)"]);
        let highlight = highlighter();
        assert_eq!(highlight("gre", &full), vec![0..3]);
        assert_eq!(highlight("gt", &full), vec![0..1, 4..5]);
        assert_eq!(highlight("", &full), Vec::<std::ops::Range<usize>>::new());
    }

    #[test]
    fn a_list_serves_longer_words_moved_past_what_was_typed_unless_incomplete() {
        let edit = |start: u32, end: u32| lsp_types::TextEdit {
            range: lsp_types::Range::new(Position::new(1, start), Position::new(1, end)),
            new_text: "greet()".to_owned(),
        };
        let mut greet = item("greet", None, false);
        greet.text_edit = Some(CompletionTextEdit::Edit(edit(4, 5)));
        greet.additional_text_edits = Some(vec![
            lsp_types::TextEdit::new(lsp_types::Range::new(Position::new(0, 0), Position::new(0, 0)), "use g;\n".into()),
            edit(9, 9),
        ]);
        let mut completions = Completions::default();
        assert!(completions.ranked(4, "g").is_none(), "nothing listed yet");
        completions.keep(4, Position::new(1, 5), "g".to_owned(), false, vec![greet, item("grab", None, false)]);
        let served = completions.ranked(4, "gre").unwrap();
        assert_eq!(labels(&served), ["greet"]);
        // The word's edit ends at the caret, which the "re" typed since moved; so did an
        // edit after it on the line, but not one on another line.
        let Some(CompletionTextEdit::Edit(text_edit)) = &served[0].text_edit else {
            panic!("the edit as given");
        };
        assert_eq!(text_edit.range, lsp_types::Range::new(Position::new(1, 4), Position::new(1, 7)));
        let additional = served[0].additional_text_edits.as_ref().unwrap();
        assert_eq!(additional[0].range.end, Position::new(0, 0));
        assert_eq!(additional[1].range, lsp_types::Range::new(Position::new(1, 11), Position::new(1, 11)));
        assert!(completions.ranked(4, "a").is_none(), "another word");
        assert!(completions.ranked(5, "gre").is_none(), "another start");
        // A row filled in stays so, brought up to the word.
        let mut filled = item("greet", None, false);
        filled.detail = Some("fn greet()".to_owned());
        filled.text_edit = Some(CompletionTextEdit::Edit(edit(4, 5)));
        assert!(completions.filled(&filled).is_none());
        let back = completions.fill(filled.clone());
        assert_eq!(back.detail.as_deref(), Some("fn greet()"));
        let kept = completions.filled(&filled).unwrap();
        let Some(CompletionTextEdit::Edit(text_edit)) = &kept.text_edit else {
            panic!("the edit as given");
        };
        assert_eq!(text_edit.range.end, Position::new(1, 7));
        assert_eq!(completions.ranked(4, "gree").unwrap()[0].detail.as_deref(), Some("fn greet()"));
        let mut incomplete = Completions::default();
        incomplete.keep(4, Position::new(1, 5), "g".to_owned(), true, Vec::new());
        assert!(incomplete.ranked(4, "gre").is_none(), "incomplete: asked again");
        assert!(incomplete.incomplete_at(4));
    }
}
