//! A symbol renamed in place: the server says what's at the caret and what it's called
//! (`textDocument/prepareRename`), when it can, else it's the word there; the new name
//! typed over it goes to `textDocument/rename`, whose edit lands in this file's editor and
//! in the other files, on disk.

use std::ops::Range;

use colosso_lsp::lsp_types::request::{PrepareRenameRequest, Rename};
use colosso_lsp::lsp_types::{
    OneOf, PrepareRenameResponse, RenameOptions, RenameParams, WorkDoneProgressParams,
};
use gpui_kit::component::input::{EditorState, Rope, RopeExt as _};
use gpui_kit::{App, AppContext as _, Entity, Task, Window};

use super::edits;
use super::providers::{Asked, Hooks};

/// The symbol to rename, as the server has it, or the text.
#[derive(Debug)]
pub struct Prepared {
    /// Its bytes in the editor's text at `version`, which the field sits over.
    pub range: Range<usize>,
    /// The editor's version of the text `range` is in: the text may be typed into while
    /// the server's asked, and the range is carried past that when the field's shown.
    pub version: u64,
    /// What it's called: the field starts with it.
    pub name: String,
}

impl Hooks {
    /// What's at `offset` of `rope` to rename, if the server renames and there's something
    /// there: as the server says, when it prepares renames; else the word at the caret.
    pub(super) fn prepare_rename(
        &self,
        rope: &Rope,
        offset: usize,
        cx: &App,
    ) -> Task<Option<Prepared>> {
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(None);
        };
        let provider = (self.capabilities(cx)).and_then(|capabilities| capabilities.rename_provider.clone());
        let prepares = match provider {
            None | Some(OneOf::Left(false)) => {
                log::info!("{} doesn't rename", self.kind.name());
                return Task::ready(None);
            }
            Some(OneOf::Left(true)) => false,
            Some(OneOf::Right(RenameOptions { prepare_provider, .. })) => prepare_provider == Some(true),
        };
        let word = rope.word_range(offset);
        if !prepares {
            return Task::ready(word.and_then(|range| named(&asked, range)));
        }
        cx.background_spawn(async move {
            let answer = asked.ask::<PrepareRenameRequest>(&asked.at(offset)).await.flatten();
            match answer? {
                PrepareRenameResponse::Range(range) => named(&asked, asked.bytes(range)),
                PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => Some(Prepared {
                    range: asked.bytes(range),
                    version: asked.version(),
                    name: placeholder,
                }),
                PrepareRenameResponse::DefaultBehavior { .. } => word.and_then(|range| named(&asked, range)),
            }
        })
    }

    /// Renames what's at `offset` of `editor`'s text to `name`: the server's edit lands in
    /// the editor, and in the other files on disk. On its own, once begun: an edit isn't
    /// dropped half-way for a later question.
    pub(super) fn rename(
        &self,
        editor: &Entity<EditorState>,
        offset: usize,
        name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(asked) = self.ask(editor.read(cx).text(), cx) else {
            return;
        };
        let (hooks, editor) = (self.clone(), editor.downgrade());
        window
            .spawn(cx, async move |cx| {
                let what = format!("renaming to `{name}`");
                let (asked, edit) = cx
                    .background_spawn(async move {
                        let params = RenameParams {
                            text_document_position: asked.at(offset),
                            new_name: name,
                            work_done_progress_params: WorkDoneProgressParams::default(),
                        };
                        let edit = asked.ask::<Rename>(&params).await.flatten();
                        (asked, edit)
                    })
                    .await;
                let Some(edit) = edit else {
                    log::info!("{what}: the server made no edit");
                    return;
                };
                if let Err(error) = edits::land(&what, edit, &asked, &hooks, &editor, cx).await {
                    log::warn!("{what}: {error}");
                }
            })
            .detach();
    }
}

/// The symbol at bytes `range` of the text, called what's there.
fn named(asked: &Asked, range: Range<usize>) -> Option<Prepared> {
    Some(Prepared {
        name: asked.slice(range.clone())?.to_owned(),
        version: asked.version(),
        range,
    })
}
