use crate::input::EditorMode;
use anyhow::Result;
use gpui::{App, Context, EntityInputHandler, Pixels, Task, Window, px};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, InlineCompletionContext,
    InlineCompletionItem, InlineCompletionResponse, InlineCompletionTriggerKind,
};
use ropey::Rope;
use std::{ops::Range, rc::Rc, time::Duration};

use crate::input::{InputBaseState, RopeExt as _, ShowCompletions, is_identifier_char};

/// Default debounce duration for inline completions.
const DEFAULT_INLINE_COMPLETION_DEBOUNCE: Duration = Duration::from_millis(300);

/// Decides which bytes of a completion label the menu paints as the match.
///
/// `query` is the word typed since the menu opened. Returned ranges are byte
/// offsets into [`CompletionItem::label`].
pub type CompletionHighlighter = dyn Fn(&str, &CompletionItem) -> Vec<Range<usize>>;

/// Highlight a prefix of the completion label, matching today's menu.
///
/// The painted length is [`CompletionItem::filter_text`] when the item sets
/// one, otherwise the query. It is shortened so the range never splits a
/// character. An empty length paints nothing.
pub fn highlight_prefix(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    let matched_len = item
        .filter_text
        .as_ref()
        .map(|text| text.len())
        .unwrap_or(query.len())
        .min(item.label.len());
    let end = item.label.floor_char_boundary(matched_len);
    if end == 0 { Vec::new() } else { vec![0..end] }
}

/// Highlight the first case-insensitive occurrence of `query` in the label.
///
/// An empty query, or a query that does not occur as a contiguous span,
/// paints nothing.
pub fn highlight_contains(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    let query: Vec<char> = query.chars().collect();
    if query.is_empty() {
        return Vec::new();
    }

    let label: Vec<(usize, char)> = item.label.char_indices().collect();
    let Some(last_start) = label.len().checked_sub(query.len()) else {
        return Vec::new();
    };

    for start in 0..=last_start {
        let matched = query
            .iter()
            .zip(label[start..].iter())
            .all(|(query_char, (_, label_char))| chars_eq_ignore_case(*query_char, *label_char));
        if !matched {
            continue;
        }

        let byte_start = label[start].0;
        let (last_index, last_char) = label[start + query.len() - 1];
        return vec![byte_start..last_index + last_char.len_utf8()];
    }
    Vec::new()
}

/// Highlight each character of `query` in order, skipping characters in between.
///
/// Matching is case-insensitive. Adjacent hits merge into one range, so a
/// contiguous match still paints as a single span. A query that cannot be
/// matched in full paints nothing.
pub fn highlight_characters(query: &str, item: &CompletionItem) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut rest = item.label.as_str();
    let mut base = 0usize;
    for query_char in query.chars() {
        let Some((index, matched)) = rest
            .char_indices()
            .find(|(_, label_char)| chars_eq_ignore_case(query_char, *label_char))
        else {
            return Vec::new();
        };

        let start = base + index;
        let end = start + matched.len_utf8();
        let next = index + matched.len_utf8();
        base += next;
        rest = &rest[next..];
        if let Some(last) = ranges.last_mut() {
            if last.end == start {
                last.end = end;
                continue;
            }
        }
        ranges.push(start..end);
    }
    ranges
}

fn chars_eq_ignore_case(left: char, right: char) -> bool {
    left == right
        || left.eq_ignore_ascii_case(&right)
        || left.to_lowercase().eq(right.to_lowercase())
}

/// Display options for the LSP completion popover.
///
/// Accessed through [`super::Lsp::completion_menu`] so embedders can tweak the
/// popover without growing the [`InputBaseState`] API. Label highlighting is
/// configured separately, on [`super::Lsp::completion_highlighter`].
///
/// ```
/// use gpui_base::input::CompletionMenuOptions;
/// let options = CompletionMenuOptions {
///     max_width: gpui::px(480.),
/// };
/// let _copied = options;
/// let _again = options;
/// ```
#[derive(Debug, Clone, Copy)]
pub struct CompletionMenuOptions {
    /// Maximum width of the popover.
    ///
    /// Defaults to 320 px, which is fine for most identifiers but can
    /// truncate longer labels. Widen this when hosting an editor that
    /// surfaces long completion labels.
    pub max_width: Pixels,
}

impl Default for CompletionMenuOptions {
    fn default() -> Self {
        Self {
            max_width: px(320.),
        }
    }
}

/// A trait for providing code completions based on the current input state and context.
pub trait CompletionProvider {
    /// Fetches completions based on the given byte offset.
    ///
    /// - The `offset` is in bytes of current cursor.
    ///
    /// textDocument/completion
    ///
    /// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_completion
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>>;

    /// Fetches an inline completion suggestion for the given position.
    ///
    /// This is called after a debounce period when the user stops typing.
    /// The provider can analyze the text and cursor position to determine
    /// what inline completion suggestion to show.
    ///
    ///
    /// # Arguments
    /// * `rope` - The current text content
    /// * `offset` - The cursor position in bytes
    ///
    /// textDocument/inlineCompletion
    ///
    /// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification/#textDocument_inlineCompletion
    fn inline_completion(
        &self,
        _rope: &Rope,
        _offset: usize,
        _trigger: InlineCompletionContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<InlineCompletionResponse>> {
        Task::ready(Ok(InlineCompletionResponse::Array(vec![])))
    }

    /// Returns the debounce duration for inline completions.
    ///
    /// Default: 300ms
    #[inline]
    fn inline_completion_debounce(&self) -> Duration {
        DEFAULT_INLINE_COMPLETION_DEBOUNCE
    }

    /// Fills in `item`: its documentation, and the edits that come with it.
    /// The list arrives light, and a row is filled in once it's looked at.
    ///
    /// completionItem/resolve
    ///
    /// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#completionItem_resolve
    fn resolve(&self, item: CompletionItem, _: &mut App) -> Task<Result<CompletionItem>> {
        Task::ready(Ok(item))
    }

    /// Determines if the completion should be triggered based on the given byte offset.
    ///
    /// This is called on the main thread.
    fn is_completion_trigger(&self, offset: usize, new_text: &str, cx: &mut App) -> bool;
}

pub(crate) struct InlineCompletion {
    /// Completion item to display as an inline completion suggestion
    pub(crate) item: Option<InlineCompletionItem>,
    /// Task for debouncing inline completion requests
    pub(crate) task: Task<Result<InlineCompletionResponse>>,
}

impl Default for InlineCompletion {
    fn default() -> Self {
        Self {
            item: None,
            task: Task::ready(Ok(InlineCompletionResponse::Array(vec![]))),
        }
    }
}

impl InputBaseState<EditorMode> {
    pub(crate) fn handle_completion_trigger(
        &mut self,
        _range: &Range<usize>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.completion_inserting {
            return;
        }

        let Some(provider) = self.extras.lsp.completion_provider.clone() else {
            return;
        };

        // Always schedule inline completion (debounced).
        // It will check if menu is open before showing the suggestion.
        self.schedule_inline_completion(window, cx);

        let new_offset = self.cursor();
        // Measure the inserted text in the current document. The replaced range
        // uses pre-edit coordinates, which preceding multi-cursor edits can
        // shift. The active caret ends immediately after the normalized input,
        // including selection replacements and IME commits.
        let Some(start) = new_offset.checked_sub(new_text.len()) else {
            return;
        };

        if !provider.is_completion_trigger(start, new_text, cx) {
            return;
        }

        // `trigger_start_offset` latches where the word a menu was opened for
        // begins, so later keystrokes refine the same query instead of starting
        // over at each character. It only describes this edit while the edit
        // continues that word: the document between the latch and the edit
        // must still read as a prefix of the last query. Deleting back into
        // the word keeps it; typing somewhere else, or into a document that
        // has since been replaced, starts a new query at this edit instead of
        // handing the provider text the user never typed as a prefix. A
        // trigger character that isn't a word's, a `.` say, begins a new,
        // empty word after it.
        let word = new_text.chars().all(is_identifier_char);
        let completion = &self.extras.context_menu_content.completion;
        let latched = completion.trigger_start_offset.filter(|&latched| {
            word && latched <= start
                && start <= latched + completion.query.len()
                && self.text.is_char_boundary(latched)
                && self.text.is_char_boundary(start)
                && completion
                    .query
                    .starts_with(self.text.slice(latched..start).to_string().as_str())
        });
        let start_offset = if word { latched.unwrap_or(start) } else { new_offset };
        if new_offset < start_offset {
            return;
        }

        let query = self
            .text_for_range(
                self.range_to_utf16(&(start_offset..new_offset)),
                &mut None,
                window,
                cx,
            )
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        self.request_completions(
            provider,
            start_offset,
            query,
            CompletionContext {
                trigger_kind: lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(new_text.to_owned()),
            },
            window,
            cx,
        );
    }

    pub(crate) fn on_action_show_completions(
        &mut self,
        _: &ShowCompletions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_completions(window, cx);
    }

    /// Request completions at the caret and focus the editor, without editing text.
    ///
    /// The current identifier prefix is used to refine the menu. An empty
    /// prefix still asks the provider for suggestions. There is no default
    /// keybinding; applications can dispatch [`ShowCompletions`] in the
    /// `Input && mode == editor` context or call this method from a command.
    ///
    /// Disabled/readonly editors, IME preedit, and an already-open completion
    /// menu are left alone. A new request cancels the previous overlay request
    /// and inline suggestion, and closes the code-action menu.
    pub fn show_completions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled
            || self.readonly
            || self.completion_inserting
            || self.ime_marked_range.is_some()
            || self.extras.context_menu_content.completion.open
        {
            return;
        }
        let Some(provider) = self.extras.lsp.completion_provider.clone() else {
            return;
        };

        let offset = self.cursor();
        let start_offset = self.text.identifier_start(offset);
        let query = self.text.slice(start_offset..offset).to_string();

        self.hide_context_menu(cx);
        self.clear_inline_completion(cx);
        self.focus(window, cx);

        self.request_completions(
            provider,
            start_offset,
            query,
            CompletionContext {
                trigger_kind: lsp_types::CompletionTriggerKind::INVOKED,
                trigger_character: None,
            },
            window,
            cx,
        );
    }

    /// Record the word a menu is for and ask `provider` about the caret.
    fn request_completions(
        &mut self,
        provider: Rc<dyn CompletionProvider>,
        start_offset: usize,
        query: String,
        completion_context: CompletionContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_offset = self.cursor();
        let revision = self.version();
        self.extras.context_menu_task = Task::ready(Ok(()));
        self.extras.context_menu_content.code_action.open = false;
        self.extras
            .context_menu_content
            .completion
            .trigger_start_offset = Some(start_offset);
        self.extras
            .context_menu_content
            .completion
            .query
            .clone_from(&query);

        let text = self.text.clone();
        self.extras.context_menu_task = cx.spawn_in(window, async move |editor, cx| {
            // Asked once the editor is done with the edit, so a provider that
            // keeps a server in step with the text hears of the edit first.
            let provider_responses = cx.update(|window, cx| {
                provider.completions(&text, new_offset, completion_context, window, cx)
            })?;
            let mut completions: Vec<CompletionItem> = vec![];
            if let Some(provider_responses) = provider_responses.await.ok() {
                match provider_responses {
                    CompletionResponse::Array(items) => completions.extend(items),
                    CompletionResponse::List(list) => completions.extend(list.items),
                }
            }

            editor
                .update_in(cx, |editor, window, cx| {
                    if !editor.focus_handle.is_focused(window)
                        || editor.version() != revision
                        || editor.cursor() != new_offset
                        || editor.ime_marked_range.is_some()
                        || editor.disabled
                        || editor.readonly
                    {
                        return;
                    }

                    editor.present_completion_items(start_offset, query, completions, cx);
                })
                .ok();

            Ok(())
        });
    }

    /// Row `ix` of the menu is selected: the provider fills it in, if it
    /// hasn't yet, for its documentation and the edits that come with it.
    pub fn select_completion(&mut self, ix: usize, cx: &mut Context<Self>) {
        let completion = &mut self.extras.context_menu_content.completion;
        if completion.resolved.get(ix).is_none_or(|resolved| *resolved)
            || completion.resolving == Some(ix)
        {
            return;
        }
        let Some(provider) = self.extras.lsp.completion_provider.clone() else {
            return;
        };
        let Some(item) = completion.items.get(ix).cloned() else {
            return;
        };
        let revision = completion.revision();
        completion.resolving = Some(ix);
        let resolving = provider.resolve(item, cx);
        self.extras.completion_resolving = cx.spawn(async move |editor, cx| {
            let resolved = resolving.await;
            editor.update(cx, |editor, cx| {
                let completion = &mut editor.extras.context_menu_content.completion;
                if completion.revision() != revision {
                    return;
                }
                completion.resolving = None;
                let Ok(item) = resolved else {
                    return;
                };
                if let (Some(slot), Some(resolved)) =
                    (completion.items.get_mut(ix), completion.resolved.get_mut(ix))
                {
                    *resolved = true;
                    // Filled in as it was: the menu needn't redraw.
                    if *slot != item {
                        *slot = item;
                        completion.refresh();
                        cx.notify();
                    }
                }
            })?;
            Ok(())
        });
    }

    /// Row `ix` of the menu is taken: inserted, filled in first if it hasn't
    /// been, so the edits that come with it, an import say, come too.
    pub fn accept_completion(
        &mut self,
        ix: usize,
        fallback_range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let completion = &self.extras.context_menu_content.completion;
        let Some(item) = completion.items.get(ix).cloned() else {
            return;
        };
        let resolved = completion.resolved.get(ix).is_none_or(|resolved| *resolved);
        let provider = self.extras.lsp.completion_provider.clone();
        let Some(provider) = provider.filter(|_| !resolved) else {
            self.insert_completion(&item, fallback_range, window, cx);
            return;
        };
        let version = self.version();
        let resolving = provider.resolve(item.clone(), cx);
        self.extras.completion_resolving = cx.spawn_in(window, async move |editor, cx| {
            let item = resolving.await.unwrap_or(item);
            editor.update_in(cx, |editor, window, cx| {
                // Typed into since: the row was for a text that's gone.
                if editor.version() == version {
                    editor.insert_completion(&item, fallback_range, window, cx);
                }
            })?;
            Ok(())
        });
    }

    pub(crate) fn hide_context_menu(&mut self, cx: &mut Context<Self>) {
        self.extras.context_menu_content.completion.open = false;
        self.extras.context_menu_content.code_action.open = false;
        self.extras.context_menu_task = Task::ready(Ok(()));
        cx.notify();
    }

    pub(crate) fn is_context_menu_open(&self, _cx: &gpui::App) -> bool {
        self.extras.context_menu_content.completion.open
            || self.extras.context_menu_content.code_action.open
    }

    pub(crate) fn handle_action_for_context_menu(
        &mut self,
        action: Box<dyn gpui::Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let closes_overlay =
            crate::input::Enter::is_primary(&*action) || action.partial_eq(&crate::input::Escape);
        let kind = if self.extras.context_menu_content.completion.open {
            Some(super::InputOverlayKind::Completion)
        } else if self.extras.context_menu_content.code_action.open {
            Some(super::InputOverlayKind::CodeAction)
        } else {
            None
        };
        let Some((kind, handler)) = kind.zip(self.overlay_action_handler.clone()) else {
            return false;
        };
        let handled = handler(kind, action, window, cx);
        if handled && closes_overlay {
            self.hide_context_menu(cx);
        }
        handled
    }

    /// Schedule an inline completion request after debouncing.
    pub(crate) fn schedule_inline_completion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Clear any existing inline completion on text change
        self.clear_inline_completion(cx);

        let Some(provider) = self.extras.lsp.completion_provider.clone() else {
            return;
        };

        let offset = self.cursor();
        let text = self.text.clone();
        let debounce = provider.inline_completion_debounce();
        let background_executor = cx.background_executor().clone();

        self.extras.inline_completion.task = cx.spawn_in(window, async move |editor, cx| {
            // Debounce: wait before fetching to avoid unnecessary requests while typing
            background_executor.timer(debounce).await;

            // Now fetch the inline completion after the debounce period
            let task = editor.update_in(cx, |editor, window, cx| {
                // Check if cursor has moved during debounce
                if editor.cursor() != offset {
                    return None;
                }

                // Don't fetch if completion menu is open
                if editor.is_context_menu_open(cx) {
                    return None;
                }

                let trigger = InlineCompletionContext {
                    trigger_kind: InlineCompletionTriggerKind::Automatic,
                    selected_completion_info: None,
                };

                Some(provider.inline_completion(&text, offset, trigger, window, cx))
            })?;

            let Some(task) = task else {
                return Ok(InlineCompletionResponse::Array(vec![]));
            };

            let response = task.await?;

            editor.update_in(cx, |editor, _window, cx| {
                // Only apply if cursor still hasn't moved
                if editor.cursor() != offset {
                    return;
                }

                // Don't show if completion menu opened while we were fetching
                if editor.is_context_menu_open(cx) {
                    return;
                }

                if let Some(item) = match response.clone() {
                    InlineCompletionResponse::Array(items) => items.into_iter().next(),
                    InlineCompletionResponse::List(comp_list) => comp_list.items.into_iter().next(),
                } {
                    editor.extras.inline_completion.item = Some(item);
                    cx.notify();
                }
            })?;

            Ok(response)
        });
    }

    /// Check if an inline completion suggestion is currently displayed.
    #[inline]
    pub(crate) fn has_inline_completion(&self) -> bool {
        self.extras.inline_completion.item.is_some()
    }

    /// Clear the inline completion suggestion.
    pub(crate) fn clear_inline_completion(&mut self, cx: &mut Context<Self>) {
        self.extras.inline_completion = InlineCompletion::default();
        cx.notify();
    }

    /// Accept the inline completion, inserting it at the cursor position.
    /// Returns true if a completion was accepted, false if there was none.
    pub(crate) fn accept_inline_completion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(completion_item) = self.extras.inline_completion.item.take() else {
            return false;
        };

        let cursor = self.cursor();
        let range_utf16 = self.range_to_utf16(&(cursor..cursor));
        let completion_text = completion_item.insert_text;
        self.replace_text_in_range_silent(Some(range_utf16), &completion_text, window, cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, filter_text: Option<&str>) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            filter_text: filter_text.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn prefix_uses_the_query_length_unless_filter_text_is_set() {
        assert_eq!(highlight_prefix("fl", &item("flag", None)), vec![0..2]);
        assert_eq!(highlight_prefix("fl", &item("flag", Some("f"))), vec![0..1]);
        assert_eq!(
            highlight_prefix("flagpole", &item("flag", None)),
            vec![0..4]
        );
        assert!(highlight_prefix("", &item("flag", None)).is_empty());
    }

    #[test]
    fn prefix_does_not_split_a_character() {
        assert_eq!(highlight_prefix("你", &item("你好", None)), vec![0..3]);
        assert!(highlight_prefix("a", &item("你", None)).is_empty());
    }

    #[test]
    fn contains_highlights_the_first_contiguous_match() {
        // "dialog" itself ends in "log", so that hit comes before the suffix.
        assert_eq!(
            highlight_contains("log", &item("dialog_log", None)),
            vec![3..6]
        );
        assert_eq!(
            highlight_contains("LOG", &item("dialog_log", None)),
            vec![3..6]
        );
        assert_eq!(
            highlight_contains("log", &item("catalog", None)),
            vec![4..7]
        );
        assert_eq!(
            highlight_contains("好世", &item("你好世界", None)),
            vec![3..9]
        );
        assert!(highlight_contains("log", &item("flag", None)).is_empty());
        assert!(highlight_contains("", &item("flag", None)).is_empty());
    }

    #[test]
    fn characters_highlight_gaps_and_merge_adjacent_hits() {
        assert_eq!(highlight_characters("fl", &item("flag", None)), vec![0..2]);
        assert_eq!(
            highlight_characters("fl", &item("file_loader", None)),
            vec![0..1, 2..3]
        );
        assert_eq!(
            highlight_characters("你世", &item("你好世界", None)),
            vec![0..3, 6..9]
        );
        assert!(highlight_characters("fx", &item("flag", None)).is_empty());
        assert!(highlight_characters("", &item("flag", None)).is_empty());
    }
}
