use super::*;
use crate::input::{Cursors, EditorMode};
use std::ops::Range;

use lsp_types::{CompletionItem, Hover, InsertTextFormat};

#[derive(Clone, Debug, Default)]
pub struct CompletionMenuState {
    pub open: bool,
    pub trigger_start_offset: Option<usize>,
    pub query: String,
    pub items: Vec<CompletionItem>,
    /// Which rows the provider has filled in, and the one it's filling in.
    pub(super) resolved: Vec<bool>,
    pub(super) resolving: Option<usize>,
    revision: u64,
    refreshed: u64,
}

impl CompletionMenuState {
    /// Bumped whenever the content changes.
    ///
    /// A renderer that mirrors this menu compares revisions to decide whether
    /// to rebuild, so it never has to compare the item list itself.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Bumped whenever a row is filled in where it is, the list otherwise
    /// the same: a renderer replaces its rows without losing its selection.
    pub fn refreshed(&self) -> u64 {
        self.refreshed
    }

    pub(super) fn refresh(&mut self) {
        self.refreshed = self.refreshed.wrapping_add(1);
    }

    pub(super) fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Clone, Debug, Default)]
pub struct CodeActionMenuState {
    pub open: bool,
    pub items: Vec<CodeActionItem>,
    revision: u64,
}

impl CodeActionMenuState {
    /// Bumped whenever the content changes. See [`CompletionMenuState::revision`].
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Clone, Debug)]
pub struct HoverPopoverState {
    pub symbol_range: Range<usize>,
    pub hover: Hover,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ContextMenuContent {
    pub(crate) completion: CompletionMenuState,
    pub(crate) code_action: CodeActionMenuState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOverlayKind {
    Completion,
    CodeAction,
}

impl InputBaseState<EditorMode> {
    /// Shows `items` for the word begun at `trigger_start_offset`, the first
    /// selected and so filled in.
    pub fn present_completion_items(
        &mut self,
        trigger_start_offset: usize,
        query: impl Into<String>,
        items: Vec<CompletionItem>,
        cx: &mut Context<Self>,
    ) {
        let completion = &mut self.extras.context_menu_content.completion;
        completion.trigger_start_offset = Some(trigger_start_offset);
        completion.query = query.into();
        completion.open = !items.is_empty();
        completion.resolved = vec![false; items.len()];
        completion.resolving = None;
        completion.items = items;
        completion.bump();
        cx.notify();
        self.select_completion(0, cx);
    }

    pub fn present_code_actions(&mut self, items: Vec<CodeActionItem>, cx: &mut Context<Self>) {
        self.extras.context_menu_content.code_action.items = items;
        self.extras.context_menu_content.code_action.open = !self
            .extras
            .context_menu_content
            .code_action
            .items
            .is_empty();
        self.extras.context_menu_content.code_action.bump();
        cx.notify();
    }

    pub fn present_hover(
        &mut self,
        symbol_range: Range<usize>,
        hover: Hover,
        cx: &mut Context<Self>,
    ) {
        self.extras.hover_popover = Some(HoverPopoverState {
            symbol_range,
            hover,
        });
        cx.notify();
    }

    pub fn present_diagnostic(
        &mut self,
        diagnostic: crate::input::DiagnosticEntry,
        cx: &mut Context<Self>,
    ) {
        self.diagnostic_popover = Some(Rc::new(diagnostic));
        cx.notify();
    }

    pub fn clear_diagnostic_popover(&mut self, cx: &mut Context<Self>) {
        if self.diagnostic_popover.take().is_some() {
            cx.notify();
        }
    }

    pub fn route_overlay_action(
        &mut self,
        action: Box<dyn gpui::Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.handle_action_for_context_menu(action, window, cx)
    }

    pub fn set_overlay_action_handler(
        &mut self,
        handler: impl Fn(
            InputOverlayKind,
            Box<dyn gpui::Action>,
            &mut Window,
            &mut Context<InputBaseState<EditorMode>>,
        ) -> bool
        + 'static,
    ) {
        self.overlay_action_handler = Some(Rc::new(handler));
    }

    pub fn has_overlay_action_handler(&self) -> bool {
        self.overlay_action_handler.is_some()
    }

    pub fn dismiss_completion_overlay(&mut self, cx: &mut Context<Self>) {
        if self.extras.context_menu_content.completion.open {
            self.extras.context_menu_content.completion.open = false;
            cx.notify();
        }
    }

    pub fn dismiss_code_action_overlay(&mut self, cx: &mut Context<Self>) {
        if self.extras.context_menu_content.code_action.open {
            self.extras.context_menu_content.code_action.open = false;
            cx.notify();
        }
    }

    /// Puts `item` in over the word it completes, or over `fallback_range`,
    /// the word the menu was opened on, when the item doesn't say; with the
    /// edits that come with it, an import say, as one undo step. A snippet
    /// goes in as plain text, its first stop selected and the rest for Tab.
    pub fn insert_completion(
        &mut self,
        item: &CompletionItem,
        fallback_range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (range, new_text) = match (&item.text_edit, &item.insert_text) {
            (Some(lsp_types::CompletionTextEdit::Edit(edit)), _) => {
                (self.lsp_range_to_offsets(&edit.range), &edit.new_text)
            }
            (Some(lsp_types::CompletionTextEdit::InsertAndReplace(edit)), _) => {
                (self.lsp_range_to_offsets(&edit.replace), &edit.new_text)
            }
            (None, Some(insert_text)) => (fallback_range, insert_text),
            (None, None) => (fallback_range, &item.label),
        };
        let snippet = if item.insert_text_format == Some(InsertTextFormat::SNIPPET) {
            Snippet::parse(new_text)
        } else {
            Snippet {
                text: new_text.clone(),
                stops: vec![vec![new_text.len()..new_text.len()]],
            }
        };

        let mut edits = vec![(range.clone(), snippet.text.as_str())];
        // How far the edits before the word move it; one over the word is
        // dropped, as the two can't both go in.
        let mut shift = 0isize;
        for edit in item.additional_text_edits.iter().flatten() {
            let elsewhere = self.lsp_range_to_offsets(&edit.range);
            if elsewhere.end <= range.start {
                shift += edit.new_text.len() as isize - elsewhere.len() as isize;
            } else if elsewhere.start < range.end {
                continue;
            }
            edits.push((elsewhere, edit.new_text.as_str()));
        }
        // An edit at the word's start goes in before the word, as given first.
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        self.completion_inserting = true;
        self.replace_text_in_ranges(&edits, Cursors::Following, window, cx);
        self.completion_inserting = false;

        let start = range.start.saturating_add_signed(shift);
        let places = |stop: &Vec<Range<usize>>| -> Vec<Range<usize>> {
            stop.iter()
                .map(|place| start + place.start..start + place.end)
                .collect()
        };
        self.select_places(&places(&snippet.stops[0]), cx);
        self.extras.snippet = (snippet.stops.len() > 1).then(|| SnippetSession {
            stops: snippet.stops.iter().map(places).collect(),
            current: 0,
            version: self.version(),
        });
        self.focus(window, cx);
    }

    fn lsp_range_to_offsets(&self, range: &lsp_types::Range) -> Range<usize> {
        self.text.position_to_offset(&range.start)..self.text.position_to_offset(&range.end)
    }

    /// Selects `places`, a cursor at each.
    fn select_places(&mut self, places: &[Range<usize>], cx: &mut Context<Self>) {
        let Some((first, rest)) = places.split_first() else {
            return;
        };
        self.set_selected_range(first.clone(), cx);
        for place in rest {
            self.add_selection(place.clone(), cx);
        }
    }

    /// Tab in a snippet: moves to its next stop, or back with `backwards`,
    /// selecting it everywhere it is. The snippet is over once its last stop
    /// is reached, or the text a stop was in is gone; then this is false and
    /// Tab does what it usually does.
    pub(crate) fn snippet_tab(&mut self, backwards: bool, cx: &mut Context<Self>) -> bool {
        let Some(mut session) = self.extras.snippet.take() else {
            return false;
        };
        let next = if backwards {
            session.current.saturating_sub(1)
        } else {
            session.current + 1
        };
        let places: Option<Vec<Range<usize>>> = (session.stops[next].iter())
            .map(|place| self.translate_range(place.clone(), session.version))
            .collect();
        let Some(places) = places else {
            return false;
        };
        self.select_places(&places, cx);
        if next + 1 < session.stops.len() {
            session.current = next;
            self.extras.snippet = Some(session);
        }
        true
    }

    #[doc(hidden)]
    pub fn completion_menu_state(&self) -> &CompletionMenuState {
        &self.extras.context_menu_content.completion
    }

    #[doc(hidden)]
    pub fn code_action_menu_state(&self) -> &CodeActionMenuState {
        &self.extras.context_menu_content.code_action
    }

    pub fn hover_popover(&self) -> Option<&HoverPopoverState> {
        self.extras.hover_popover.as_ref()
    }

    pub fn dismiss_lsp_overlays(&mut self, cx: &mut Context<Self>) {
        self.hide_context_menu(cx);
        self.clear_hover_state(cx);
    }
}
