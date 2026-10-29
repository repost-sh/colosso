//! The editor's hooks into a file's server: where a symbol is defined, what it is, and what's
//! wrong with the file. Positions go to the server through colosso-lsp's line index, in the
//! encoding the server chose, and come back as the editor's own.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use colosso_lsp::lsp_types::request::{GotoDefinition, HoverRequest, Request};
use colosso_lsp::lsp_types::{
    self, GotoDefinitionParams, GotoDefinitionResponse, HoverParams, LocationLink,
    TextDocumentIdentifier, TextDocumentPositionParams,
};
use colosso_lsp::{Client, Encoding, uri};
use gpui_kit::component::input::{
    DefinitionProvider, EditorState, HoverProvider, Rope, RopeExt as _,
};
use gpui_kit::{App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity, Window};

use super::diagnostics;
use super::navigate::show_document;
use super::server::Server;
use super::sync::Shown;
use crate::checkout::Checkout;
use crate::files::FileEditor;

/// A file shown in the viewer, given to its language's server: the hooks its editor is
/// equipped with, and the diagnostics last put into it.
pub struct Attached {
    server: Entity<Server>,
    /// The checkout the file is in, where a definition in another file is read.
    checkout: WeakEntity<Checkout>,
    path: Arc<Path>,
    /// When the diagnostics put into the editor were published; once in, the editor moves
    /// them along with its edits.
    applied: Option<u64>,
    _server: Subscription,
}

impl Attached {
    pub(super) fn new(
        server: Entity<Server>,
        checkout: WeakEntity<Checkout>,
        path: Arc<Path>,
        cx: &mut Context<FileEditor>,
    ) -> Self {
        let _server = cx.observe(&server, |view, _, cx| view.language_changed(cx));
        Attached {
            server,
            checkout,
            path,
            applied: None,
            _server,
        }
    }

    /// Gives `editor`, just made for the file, its hooks into the server, and the server
    /// its text; `view` is the viewer showing it, which shows a definition found in another
    /// file.
    pub fn equip(
        &mut self,
        editor: &Entity<EditorState>,
        view: WeakEntity<FileEditor>,
        cx: &mut App,
    ) {
        let hooks = Rc::new(Hooks {
            server: self.server.downgrade(),
            path: self.path.clone(),
        });
        let server = self.server.downgrade();
        let show_document = show_document(server, self.checkout.clone(), self.path.clone(), view);
        editor.update(cx, |editor, _| {
            let lsp = editor.lsp_mut();
            lsp.definition_provider = Some(hooks.clone());
            lsp.hover_provider = Some(hooks);
            lsp.show_document = Some(show_document);
        });
        self.server
            .update(cx, |server, cx| server.show(&self.path, editor, cx));
        self.applied = None;
    }

    /// `editor` edited the file's text: the server hears of the edits.
    pub fn changed(&self, editor: &Entity<EditorState>, cx: &mut App) {
        self.server
            .update(cx, |server, cx| server.changed(&self.path, editor, cx));
    }

    /// Puts the diagnostics published for the file into `editor`, unless it has them
    /// already.
    pub fn refresh(&mut self, editor: &Entity<EditorState>, cx: &mut App) {
        let (published, diagnostics) = self.server.read(cx).published.get(&self.path);
        if self.applied == Some(published) {
            return;
        }
        editor.update(cx, |editor, cx| diagnostics::show(editor, &diagnostics, cx));
        self.applied = Some(published);
    }
}

/// The definition and hover providers of one file's editor.
struct Hooks {
    server: WeakEntity<Server>,
    path: Arc<Path>,
}

/// A position in the file as the server knows it, with what's needed to convert its answer.
struct Asked {
    client: Client,
    at: TextDocumentPositionParams,
    shown: Shown,
    encoding: Encoding,
    rope: Rope,
}

impl Hooks {
    /// The byte `offset` in `rope` as the server knows it, if the server's running and has
    /// the text the editor shows.
    fn ask(&self, rope: &Rope, offset: usize, cx: &App) -> Option<Asked> {
        let server = self.server.upgrade()?;
        let server = server.read(cx);
        let client = server.client.clone()?;
        let open = server.documents.get(&self.path).filter(|open| open.sent)?;
        let shown = open.shown.clone()?;
        // Another version than the editor's, and the positions would mean nothing.
        if shown.text.as_str().len() != rope.len() {
            return None;
        }
        let position = shown.text.position(offset, server.encoding);
        Some(Asked {
            client,
            at: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier::new(open.lsp.uri().clone()),
                position,
            },
            shown,
            encoding: server.encoding,
            rope: rope.clone(),
        })
    }
}

impl Asked {
    /// Asks `R`, again while the server says the workspace changed under it, and logs how
    /// long the answer took. A failure is logged and taken as no answer.
    async fn ask<R: Request>(&self, params: &R::Params) -> Option<R::Result> {
        let asked = Instant::now();
        let answer = self.client.request_retrying::<R>(params).await;
        let took = asked.elapsed().as_secs_f64() * 1000.;
        match answer {
            Ok(answer) => {
                log::debug!("{} answered in {took:.0} ms", R::METHOD);
                Some(answer)
            }
            Err(error) => {
                log::debug!("{} failed after {took:.0} ms: {error}", R::METHOD);
                None
            }
        }
    }

    /// A range in this file, from the server's encoding to the editor's positions.
    fn for_editor(&self, range: lsp_types::Range) -> lsp_types::Range {
        let bytes = self.shown.text.offsets(range, self.encoding);
        lsp_types::Range::new(
            self.rope.offset_to_position(bytes.start),
            self.rope.offset_to_position(bytes.end),
        )
    }

    /// A link the server gave, its ranges in this file made the editor's own; another file's
    /// stay the server's, to be converted once that file is read.
    fn link_for_editor(&self, mut link: LocationLink, path: &Path) -> LocationLink {
        link.origin_selection_range = link
            .origin_selection_range
            .map(|range| self.for_editor(range));
        if uri::to_path(&link.target_uri).as_deref() == Some(path) {
            link.target_range = self.for_editor(link.target_range);
            link.target_selection_range = self.for_editor(link.target_selection_range);
        }
        link
    }
}

impl DefinitionProvider for Hooks {
    fn definitions(
        &self,
        rope: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Vec<LocationLink>>> {
        let Some(asked) = self.ask(rope, offset, cx) else {
            return Task::ready(Ok(Vec::new()));
        };
        let path = self.path.clone();
        // Dropped by the editor once it asks again, which cancels the request.
        cx.background_spawn(async move {
            let params = GotoDefinitionParams {
                text_document_position_params: asked.at.clone(),
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
                partial_result_params: lsp_types::PartialResultParams::default(),
            };
            let answer = asked.ask::<GotoDefinition>(&params).await.flatten();
            Ok((links(answer).into_iter())
                .map(|link| asked.link_for_editor(link, &path))
                .collect())
        })
    }
}

impl HoverProvider for Hooks {
    fn hover(
        &self,
        rope: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Option<lsp_types::Hover>>> {
        let Some(asked) = self.ask(rope, offset, cx) else {
            return Task::ready(Ok(None));
        };
        cx.background_spawn(async move {
            let params = HoverParams {
                text_document_position_params: asked.at.clone(),
                work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
            };
            let hover = asked.ask::<HoverRequest>(&params).await.flatten();
            Ok(hover.map(|mut hover| {
                hover.range = hover.range.map(|range| asked.for_editor(range));
                hover
            }))
        })
    }
}

/// Every place a definition answer names, as links. Several are possible; the editor goes
/// to the first.
fn links(answer: Option<GotoDefinitionResponse>) -> Vec<LocationLink> {
    let link = |location: lsp_types::Location| LocationLink {
        origin_selection_range: None,
        target_uri: location.uri,
        target_range: location.range,
        target_selection_range: location.range,
    };
    match answer {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => vec![link(location)],
        Some(GotoDefinitionResponse::Array(locations)) => locations.into_iter().map(link).collect(),
        Some(GotoDefinitionResponse::Link(links)) => links,
    }
}
