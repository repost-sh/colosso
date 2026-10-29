use anyhow::Result;
use gpui::{App, Context, Hsla, SharedString, Task, Window};
use ropey::Rope;
use std::rc::Rc;

use crate::input::{EditorMode, InputBaseState, RopeExt};

mod code_actions;
mod completions;
mod definitions;
mod document_colors;
mod hover;
mod inlay_hints;
mod overlay;
mod semantic_tokens;
mod snippet;

pub use code_actions::*;
pub use completions::*;
pub use definitions::*;
pub use document_colors::*;
pub use hover::*;
pub use inlay_hints::*;
pub use overlay::*;
pub use semantic_tokens::*;
pub(crate) use snippet::{Snippet, SnippetSession};

/// Host hook to show a document when following an LSP location
/// (Go to Definition), modeled after the `window/showDocument` request.
///
/// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#window_showDocument
///
/// Called before the built-in behavior. Return `true` if the host has shown
/// the document (e.g. opened a docs window for a virtual/external URI);
/// return `false` to fall through to the default handling (`external` URIs
/// open in the browser, anything else jumps within the current document).
pub type ShowDocumentHandler =
    Rc<dyn Fn(&lsp_types::ShowDocumentParams, &mut Window, &mut App) -> bool>;

/// LSP ServerCapabilities
///
/// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#serverCapabilities
pub struct Lsp {
    /// The completion provider.
    pub completion_provider: Option<Rc<dyn CompletionProvider>>,
    /// The code action providers.
    pub code_action_providers: Vec<Rc<dyn CodeActionProvider>>,
    /// The hover provider.
    pub hover_provider: Option<Rc<dyn HoverProvider>>,
    /// The definition provider.
    pub definition_provider: Option<Rc<dyn DefinitionProvider>>,
    /// The document color provider.
    pub document_color_provider: Option<Rc<dyn DocumentColorProvider>>,
    /// The range semantic tokens provider.
    pub semantic_tokens_provider: Option<Rc<dyn DocumentRangeSemanticTokensProvider>>,
    /// The inlay hint provider.
    pub inlay_hint_provider: Option<Rc<dyn InlayHintProvider>>,
    /// Optional host hook to show documents for Go to Definition locations,
    /// following the `window/showDocument` request (see [`ShowDocumentHandler`]).
    ///
    /// https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#window_showDocument
    pub show_document: Option<ShowDocumentHandler>,

    /// Display options for the completion popover.
    pub completion_menu: CompletionMenuOptions,

    /// Decides which bytes of each completion label the menu paints.
    ///
    /// Defaults to [`highlight_prefix`]. Replace it to paint a contiguous
    /// match, a character-by-character match, or ranges the host computed.
    completion_highlighter: Rc<CompletionHighlighter>,

    pub(crate) document_colors: Vec<(lsp_types::Range, Hsla)>,
    /// Cached semantic tokens as absolute position ranges + theme token-type
    /// names. Color is resolved from the name at paint time so theme switches
    /// take effect without a refetch.
    pub(crate) semantic_tokens: Vec<(lsp_types::Range, SharedString)>,
    /// The inlay hints of the text in view, see [`Inlays`].
    pub(crate) inlays: Inlays,
    pub(crate) _hover_task: Task<Result<()>>,
    pub(crate) _definition_task: Task<Result<()>>,
    pub(crate) _document_color_task: Task<()>,
    pub(crate) _semantic_tokens_task: Task<()>,
    pub(crate) _inlay_hints_task: Task<()>,
}

impl Default for Lsp {
    fn default() -> Self {
        Self {
            completion_provider: None,
            code_action_providers: vec![],
            hover_provider: None,
            definition_provider: None,
            document_color_provider: None,
            completion_menu: CompletionMenuOptions::default(),
            completion_highlighter: Rc::new(highlight_prefix),
            semantic_tokens_provider: None,
            inlay_hint_provider: None,
            show_document: None,
            document_colors: vec![],
            semantic_tokens: vec![],
            inlays: Inlays::default(),
            _hover_task: Task::ready(Ok(())),
            _definition_task: Task::ready(Ok(())),
            _document_color_task: Task::ready(()),
            _semantic_tokens_task: Task::ready(()),
            _inlay_hints_task: Task::ready(()),
        }
    }
}

impl Lsp {
    /// Set the completion label highlighter when constructing the LSP configuration.
    pub fn with_completion_highlighter(mut self, highlighter: Rc<CompletionHighlighter>) -> Self {
        self.set_completion_highlighter(highlighter);
        self
    }

    /// Return the completion label highlighter.
    pub fn completion_highlighter(&self) -> &Rc<CompletionHighlighter> {
        &self.completion_highlighter
    }

    /// Replace the completion label highlighter.
    ///
    /// Notify the editor after updating its LSP configuration to repaint an open menu.
    pub fn set_completion_highlighter(&mut self, highlighter: Rc<CompletionHighlighter>) {
        self.completion_highlighter = highlighter;
    }

    /// Update the LSP when the text changes.
    pub(crate) fn update(
        &mut self,
        text: &Rope,
        window: &mut Window,
        cx: &mut Context<InputBaseState<EditorMode>>,
    ) {
        self.update_document_colors(text, window, cx);
        self.update_semantic_tokens(text, window, cx);
        // Asked again at the next frame, which knows the rows in view.
        self.outdate_inlays();
    }

    /// Reset all LSP states.
    pub(crate) fn reset(&mut self) {
        self.document_colors.clear();
        self.semantic_tokens.clear();
        self.inlays = Inlays::default();
        self._inlay_hints_task = Task::ready(());
        self._hover_task = Task::ready(Ok(()));
        self._definition_task = Task::ready(Ok(()));
        self._document_color_task = Task::ready(());
        self._semantic_tokens_task = Task::ready(());
    }
}

impl InputBaseState<EditorMode> {
    /// Apply a list of [`lsp_types::TextEdit`] to mutate the text.
    ///
    /// As LSP has it, every range is in the text as it is now, and edits at
    /// the same position land in the order given. They are applied as one
    /// batch, so they undo as one, and the selection stays where it was,
    /// moved along by the edits.
    pub fn apply_lsp_edits(
        &mut self,
        text_edits: &Vec<lsp_types::TextEdit>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let edits: Vec<_> = text_edits
            .iter()
            .map(|edit| {
                let start = self.text.position_to_offset(&edit.range.start);
                let end = self.text.position_to_offset(&edit.range.end);
                (start..end, edit.new_text.as_str())
            })
            .collect();
        self.edit(&edits, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::EntityInputHandler as _;

    #[gpui::test]
    fn lsp_edits_at_one_position_land_in_their_order(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (state, cx) = cx.add_window_view(|window, cx| {
            crate::input::EditorState::new(window, cx).default_value("abcdef")
        });
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_selected_range(4..4, cx);
                // Two inserts at one position, and a replacement after them.
                state.apply_lsp_edits(
                    &vec![edit(3, 3, "x"), edit(3, 3, "y"), edit(5, 6, "Z")],
                    window,
                    cx,
                );
                assert_eq!(state.value(), "abcxydeZ");
                assert_eq!(state.selected_range(), 6..6);
                // An insert followed by a replacement at the same position.
                state.apply_lsp_edits(&vec![edit(3, 3, "["), edit(3, 5, "-")], window, cx);
                assert_eq!(state.value(), "abc[-deZ");
                assert_eq!(state.selected_range(), 6..6);
            });
        });
    }

    fn edit(start: u32, end: u32, text: &str) -> lsp_types::TextEdit {
        lsp_types::TextEdit {
            range: lsp_types::Range::new(
                lsp_types::Position::new(0, start),
                lsp_types::Position::new(0, end),
            ),
            new_text: text.into(),
        }
    }

    #[gpui::test]
    fn a_snippet_completion_goes_in_with_its_edits_and_tab_visits_its_stops(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(crate::init);
        let (state, cx) = cx.add_window_view(|window, cx| {
            crate::input::EditorState::new(window, cx).default_value("gr")
        });
        let item = lsp_types::CompletionItem {
            label: "greet".into(),
            text_edit: Some(lsp_types::CompletionTextEdit::Edit(edit(
                0,
                2,
                "greet(${1:name}, ${2:again})$0",
            ))),
            insert_text_format: Some(lsp_types::InsertTextFormat::SNIPPET),
            // An import above the word, as a server adds one.
            additional_text_edits: Some(vec![edit(0, 0, "use hi;\n")]),
            ..Default::default()
        };
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.set_selected_range(2..2, cx);
                state.insert_completion(&item, 0..2, window, cx);
                assert_eq!(state.value(), "use hi;\ngreet(name, again)");
                // The first stop is selected, after the import.
                assert_eq!(state.selected_range(), 14..18);
                // Typed over, as the user does.
                state.replace_text_in_range(None, "you", window, cx);
                assert_eq!(state.value(), "use hi;\ngreet(you, again)");
                // Tab: the next stop, where it is now; Shift-Tab: back to the typed one.
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert_eq!(state.selected_range(), 19..24);
                state.outdent_inline(&crate::input::OutdentInline, window, cx);
                assert_eq!(state.selected_range(), 14..17);
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert_eq!(state.selected_range(), 19..24);
                // Onto the last stop, and the snippet is over: Tab then indents.
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert_eq!(state.selected_range(), 25..25);
                assert!(state.extras.snippet.is_none());
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert!(state.value().starts_with("use hi;\ngreet(you, again)"));
                assert!(state.value().len() > "use hi;\ngreet(you, again)".len());
                // One undo step for the completion and its import.
                state.undo(&crate::input::Undo, window, cx);
                state.undo(&crate::input::Undo, window, cx);
                state.undo(&crate::input::Undo, window, cx);
                assert_eq!(state.value(), "gr");
            });
        });
    }

    #[gpui::test]
    fn escape_ends_a_snippet_and_so_does_a_text_replaced_whole(cx: &mut gpui::TestAppContext) {
        cx.update(crate::init);
        let (state, cx) = cx.add_window_view(|window, cx| {
            crate::input::EditorState::new(window, cx).default_value("")
        });
        let item = lsp_types::CompletionItem {
            label: "pair".into(),
            insert_text: Some("${1:a} = $1; $2".into()),
            insert_text_format: Some(lsp_types::InsertTextFormat::SNIPPET),
            ..Default::default()
        };
        cx.update(|window, cx| {
            state.update(cx, |state, cx| {
                state.insert_completion(&item, 0..0, window, cx);
                assert_eq!(state.value(), "a = ; ");
                // A repeated stop is selected everywhere it is.
                assert_eq!(state.selected_ranges(), vec![0..1, 4..4]);
                state.escape(&crate::input::Escape, window, cx);
                assert_eq!(state.selected_ranges(), vec![0..1]);
                assert!(state.extras.snippet.is_none());

                state.set_value("", window, cx);
                state.insert_completion(&item, 0..0, window, cx);
                assert!(state.extras.snippet.is_some());
                // Typing over a stop keeps the snippet, its other stops moved along.
                state.replace_text_in_range(None, "xy", window, cx);
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert_eq!(state.value(), "xy = xy; ");
                assert_eq!(state.selected_range(), 9..9);
                // The text replaced whole, the snippet is over, and Tab indents.
                state.set_value("new", window, cx);
                assert!(state.extras.snippet.is_none());
                state.indent_inline(&crate::input::IndentInline, window, cx);
                assert_ne!(state.value(), "new");
            });
        });
    }

    #[test]
    fn test_lsp_builder() {
        let highlighter: Rc<CompletionHighlighter> = Rc::new(highlight_contains);
        let mut lsp = Lsp::default().with_completion_highlighter(highlighter.clone());
        assert!(Rc::ptr_eq(lsp.completion_highlighter(), &highlighter));

        let replacement: Rc<CompletionHighlighter> = Rc::new(highlight_characters);
        lsp.set_completion_highlighter(replacement.clone());
        assert!(Rc::ptr_eq(lsp.completion_highlighter(), &replacement));
    }
}
