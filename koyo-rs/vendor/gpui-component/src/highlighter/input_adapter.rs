use std::{cell::RefCell, ops::Range, rc::Rc, time::Duration};

use gpui::{HighlightStyle, SharedString, Task};
use gpui_base::async_util::{CancelOnDrop, cancelled};
use gpui_base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit as BaseInputEdit, InputHighlighter,
    InputHighlighterFactory,
};
use ropey::Rope;
use tree_sitter::{InputEdit, ParseOptions, Point};

use super::{LanguageRegistry, SyntaxHighlighter, highlighter::parse_input_bytes};

pub(crate) fn input_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        LanguageRegistry::singleton().has_parser(language).then(|| {
            Box::new(TreeSitterInputHighlighter::new(language)) as Box<dyn InputHighlighter>
        })
    })
}

/// The main thread's share of an edit's reparse, the injection layers it
/// changed included. Past it the rest runs in the background.
const SYNC_PARSE_TIMEOUT: Duration = Duration::from_millis(1);
const PARSE_DEBOUNCE: Duration = Duration::from_millis(150);

struct TreeSitterInputHighlighter {
    inner: Rc<RefCell<SyntaxHighlighter>>,
    parse_task: Rc<RefCell<Option<Task<()>>>>,
}

impl TreeSitterInputHighlighter {
    fn new(language: &str) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SyntaxHighlighter::new(language))),
            parse_task: Rc::new(RefCell::new(None)),
        }
    }

    /// Settle after the edits reached the tree: drop a pending background
    /// parse when the synchronous one `completed`, or schedule one for `text`.
    fn finish_update(
        &mut self,
        completed: bool,
        text: &Rope,
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        if completed {
            self.parse_task.borrow_mut().take();
            return;
        }

        let highlighter = self.inner.clone();
        let parse_task = self.parse_task.clone();
        let language = highlighter.borrow().language().clone();
        let old_tree = highlighter.borrow().tree().cloned();
        let injection_data = highlighter.borrow().injection_parse_data();
        let stale = highlighter.borrow().stale_injections();
        let text = text.clone();
        let text_for_apply = text.clone();
        let cancel = CancelOnDrop::new();
        let parse_cancel = cancel.flag();

        let task = cx.spawn_in(window, async move |entity, cx| {
            let _cancel = cancel;
            cx.background_executor().timer(PARSE_DEBOUNCE).await;

            let result = cx
                .background_executor()
                .spawn(async move {
                    let (mut parser, grammar) =
                        LanguageRegistry::singleton().parser(&language).ok()?;
                    parser.set_language(&grammar).ok()?;
                    let mut progress = |_: &tree_sitter::ParseState| {
                        if cancelled(&parse_cancel) {
                            std::ops::ControlFlow::Break(())
                        } else {
                            std::ops::ControlFlow::Continue(())
                        }
                    };
                    let options = ParseOptions::new().progress_callback(&mut progress);
                    let tree = parser.parse_with_options(
                        &mut |offset, _| parse_input_bytes(&text, offset),
                        old_tree.as_ref(),
                        Some(options),
                    )?;
                    if cancelled(&parse_cancel) {
                        return None;
                    }
                    let injections = SyntaxHighlighter::background_injections(
                        injection_data,
                        old_tree.as_ref(),
                        &tree,
                        &text,
                        stale,
                    );
                    let folds = if folding {
                        extract_fold_ranges(&tree)
                    } else {
                        Vec::new()
                    };
                    Some((tree, injections, folds))
                })
                .await;

            if let Some((tree, injections, folds)) = result {
                highlighter
                    .borrow_mut()
                    .apply_background_tree(tree, &text_for_apply, injections);
                let _ = entity.update(cx, |state, cx| {
                    state.apply_highlighter_fold_candidates(folds, cx);
                });
            }
        });
        parse_task.borrow_mut().replace(task);
    }
}

impl SyntaxHighlighter {
    pub(crate) fn update_input(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        timeout: Option<Duration>,
    ) -> bool {
        self.update(edit.map(to_tree_sitter_edit), text, timeout)
    }
}

impl InputHighlighter for TreeSitterInputHighlighter {
    fn language(&self) -> SharedString {
        self.inner.borrow().language().clone()
    }

    fn update(
        &mut self,
        edit: Option<BaseInputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        let completed = self.inner.borrow_mut().update(
            edit.map(to_tree_sitter_edit),
            text,
            Some(SYNC_PARSE_TIMEOUT),
        );
        self.finish_update(completed, text, folding, window, cx);
    }

    fn update_batch(
        &mut self,
        edits: &[(BaseInputEdit, Rope)],
        folding: bool,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<EditorState>,
    ) {
        let Some((_, text)) = edits.last() else {
            return;
        };
        let edits: Vec<InputEdit> = edits
            .iter()
            .map(|(edit, _)| to_tree_sitter_edit(*edit))
            .collect();
        let completed =
            self.inner
                .borrow_mut()
                .update_edits(&edits, text, Some(SYNC_PARSE_TIMEOUT));
        self.finish_update(completed, text, folding, window, cx);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.inner.borrow().line_styles(range, resolver)
    }

    fn styles_changed(&mut self) {
        self.inner.borrow_mut().clear_line_styles();
    }
    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(extract_fold_ranges)
            .unwrap_or_default()
    }

    fn fold_ranges_for_edit(&self, range: Range<usize>, _: &Rope) -> Vec<FoldRange> {
        self.inner
            .borrow()
            .tree()
            .map(|tree| extract_fold_ranges_in_range(tree, range))
            .unwrap_or_default()
    }
}

fn to_tree_sitter_edit(edit: BaseInputEdit) -> InputEdit {
    InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: Point::new(edit.start_position.row, edit.start_position.column),
        old_end_position: Point::new(edit.old_end_position.row, edit.old_end_position.column),
        new_end_position: Point::new(edit.new_end_position.row, edit.new_end_position.column),
    }
}

fn extract_fold_ranges(tree: &tree_sitter::Tree) -> Vec<FoldRange> {
    extract_fold_ranges_in_range(tree, 0..usize::MAX)
}

fn extract_fold_ranges_in_range(
    tree: &tree_sitter::Tree,
    byte_range: Range<usize>,
) -> Vec<FoldRange> {
    /// Visit the named children of the node under `cursor` that intersect
    /// `bytes`. The cursor jumps to the first child reaching past
    /// `bytes.start` and stops after the last one starting before
    /// `bytes.end`, so an edit costs the depth of the tree at it rather than
    /// every top-level item of the document.
    fn collect_children(
        cursor: &mut tree_sitter::TreeCursor,
        bytes: &Range<usize>,
        ranges: &mut Vec<FoldRange>,
    ) {
        if cursor.goto_first_child_for_byte(bytes.start).is_none() {
            return;
        }
        loop {
            let node = cursor.node();
            if node.start_byte() >= bytes.end {
                break;
            }
            if node.is_named() {
                collect(cursor, bytes, ranges);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    fn collect(
        cursor: &mut tree_sitter::TreeCursor,
        bytes: &Range<usize>,
        ranges: &mut Vec<FoldRange>,
    ) {
        let node = cursor.node();
        if node.end_byte() <= bytes.start || node.start_byte() >= bytes.end {
            return;
        }
        let start = node.start_position().row;
        let end = node.end_position().row;
        if end.saturating_sub(start) < 2 {
            return;
        }
        ranges.push(FoldRange::new(start, end));
        collect_children(cursor, bytes, ranges);
    }

    let mut ranges = Vec::new();
    let mut cursor = tree.root_node().walk();
    collect_children(&mut cursor, &byte_range, &mut ranges);
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}
