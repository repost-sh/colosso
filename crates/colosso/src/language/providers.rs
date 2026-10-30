//! The editor's hooks into a file's server: where a symbol is defined, what it is, what's
//! wrong with the file, what could complete the word being typed (`completions`), and what
//! could be done about the code (`code_actions`). Positions go to the server through
//! colosso-lsp's line index, in the encoding the server chose, and come back as the editor's
//! own.

use std::cell::RefCell;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use colosso_lsp::lsp_types::request::{GotoDefinition, HoverRequest, Request};
use colosso_lsp::lsp_types::{
    self, GotoDefinitionParams, GotoDefinitionResponse, HoverParams, LocationLink, OneOf,
    ServerCapabilities, TextDocumentIdentifier, TextDocumentPositionParams,
};
use colosso_lsp::{Client, Encoding, uri};
use gpui_kit::component::input::{
    DefinitionProvider, DocumentRangeSemanticTokensProvider, EditorState, HoverProvider,
    InlayHintProvider, PickDefinitionHandler, Rope,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Subscription, Task, WeakEntity, Window, px,
};

use super::completions::Completions;
use super::locations::Found;
use super::navigate::show_document;
use super::rename::Prepared;
use super::symbols::Symbol;
use super::server::Server;
use super::sync::Shown;
use super::{Kind, completions, diagnostics};
use crate::checkout::Checkout;
use crate::files::{FileEditor, MOST_COLOURED};
use crate::settings;

/// A file shown in the viewer, given to its language's server: the hooks its editor is
/// equipped with, and the diagnostics last put into it.
pub struct Attached {
    server: Entity<Server>,
    /// The checkout the file is in, where an edit to another file is made.
    checkout: WeakEntity<Checkout>,
    path: Arc<Path>,
    /// When the diagnostics put into the editor were published; once in, the editor moves
    /// them along with its edits.
    applied: Option<u64>,
    /// The hooks its editor was equipped with, to put the inlay hint and semantic token
    /// providers in and take them out as the settings say.
    hooks: Option<Rc<Hooks>>,
    /// The inlay hints and the semantic colouring, each its setting and the server's count
    /// of its going out of date, as last followed into the editor; `None` before an editor
    /// has them.
    followed: Option<((bool, u64), (bool, u64))>,
    _server: Subscription,
    _settings: Subscription,
}

impl Attached {
    pub(super) fn new(
        server: Entity<Server>,
        checkout: WeakEntity<Checkout>,
        path: Arc<Path>,
        cx: &mut Context<FileEditor>,
    ) -> Self {
        let _server = cx.observe(&server, |view, _, cx| view.language_changed(cx));
        let _settings = settings::on_change(cx, FileEditor::language_changed);
        Attached {
            server,
            checkout,
            path,
            applied: None,
            hooks: None,
            followed: None,
            _server,
            _settings,
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
            kind: self.server.read(cx).kind(),
            server: self.server.downgrade(),
            checkout: self.checkout.clone(),
            path: self.path.clone(),
            completions: Rc::default(),
        });
        let server = self.server.downgrade();
        let show_document = show_document(server, self.path.clone(), view.clone());
        let pick_definition = pick_definition(hooks.clone(), view);
        editor.update(cx, |editor, _| {
            let lsp = editor.lsp_mut();
            lsp.definition_provider = Some(hooks.clone());
            lsp.hover_provider = Some(hooks.clone());
            lsp.completion_provider = Some(hooks.clone());
            lsp.code_action_providers = vec![hooks.clone()];
            lsp.set_completion_highlighter(completions::highlighter());
            lsp.completion_menu.max_width = px(440.);
            lsp.show_document = Some(show_document);
            lsp.pick_definition = Some(pick_definition);
        });
        self.server
            .update(cx, |server, cx| server.show(&self.path, editor, cx));
        // The inlay hint and semantic token providers go in with the next `refresh`, as
        // the settings say.
        self.applied = None;
        self.hooks = Some(hooks);
        self.followed = None;
    }

    /// Every reference to the symbol at `editor`'s caret, with its line, for the panel
    /// listing them; nothing while the server isn't running.
    pub fn references(&self, editor: &Entity<EditorState>, cx: &mut App) -> Task<Option<Found>> {
        let Some(hooks) = &self.hooks else {
            return Task::ready(None);
        };
        let (rope, offset) = {
            let editor = editor.read(cx);
            (editor.text().clone(), editor.cursor())
        };
        hooks.references(&rope, offset, cx)
    }

    /// What's at `editor`'s caret to rename, and what it's called; nothing while the
    /// server isn't running, or doesn't rename, or says there's nothing there.
    pub fn prepare_rename(&self, editor: &Entity<EditorState>, cx: &App) -> Task<Option<Prepared>> {
        let Some(hooks) = &self.hooks else {
            return Task::ready(None);
        };
        let editor = editor.read(cx);
        hooks.prepare_rename(editor.text(), editor.cursor(), cx)
    }

    /// Renames what's at `offset` of `editor`'s text to `name`: here, in the editor, and
    /// in the other files, on disk; on its own, once begun.
    pub fn rename(
        &self,
        editor: &Entity<EditorState>,
        offset: usize,
        name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(hooks) = &self.hooks {
            hooks.rename(editor, offset, name, window, cx);
        }
    }

    /// The file's symbols, for the palette; none while the server isn't running.
    pub fn document_symbols(&self, editor: &Entity<EditorState>, cx: &App) -> Task<Vec<Symbol>> {
        let Some(hooks) = &self.hooks else {
            return Task::ready(Vec::new());
        };
        hooks.document_symbols(editor.read(cx).text(), cx)
    }

    /// `editor` edited the file's text: the server hears of the edits.
    pub fn changed(&self, editor: &Entity<EditorState>, cx: &mut App) {
        self.server
            .update(cx, |server, cx| server.changed(&self.path, editor, cx));
    }

    /// Puts the diagnostics published for the file into `editor`, unless it has them
    /// already; and has its inlay hints, and its semantic tokens, fetched again when the
    /// server's went out of date, or the setting turned them on or off.
    pub fn refresh(&mut self, editor: &Entity<EditorState>, cx: &mut App) {
        let (published, diagnostics) = self.server.read(cx).published.get(&self.path);
        if self.applied != Some(published) {
            editor.update(cx, |editor, cx| diagnostics::show(editor, &diagnostics, cx));
            self.applied = Some(published);
        }
        let (settings, server) = (settings::get(cx), self.server.read(cx));
        let hints = (settings.inlay_hints, server.hints_refreshed);
        let tokens = (settings.semantic_colouring, server.tokens_refreshed);
        let before = self.followed.replace((hints, tokens));
        let hints_changed = before.map(|(hints, _)| hints) != Some(hints);
        let tokens_changed = before.map(|(_, tokens)| tokens) != Some(tokens);
        if !hints_changed && !tokens_changed {
            return;
        }
        let hooks = self.hooks.clone();
        editor.update(cx, |editor, cx| {
            // A file too big for the grammar's colours gets none from the server either.
            let coloured = tokens.0 && editor.text().len() <= MOST_COLOURED;
            let lsp = editor.lsp_mut();
            if hints_changed {
                let hooks = hooks.clone().filter(|_| hints.0);
                lsp.inlay_hint_provider = hooks.map(|hooks| -> Rc<dyn InlayHintProvider> { hooks });
                lsp.refresh_inlay_hints();
            }
            if tokens_changed {
                let hooks = hooks.filter(|_| coloured);
                lsp.semantic_tokens_provider =
                    hooks.map(|hooks| -> Rc<dyn DocumentRangeSemanticTokensProvider> { hooks });
                lsp.refresh_semantic_tokens();
            }
            cx.notify();
        });
    }
}

/// What the editor calls with several definitions of a symbol: they're listed, with their
/// lines, under `view`, for the user to pick from.
fn pick_definition(hooks: Rc<Hooks>, view: WeakEntity<FileEditor>) -> PickDefinitionHandler {
    Rc::new(move |links, rope, symbol, _, cx| {
        let found = hooks.definitions(links, rope, symbol, cx);
        view.update(cx, |view, cx| view.list(found, cx)).ok();
    })
}

/// The providers of one file's editor. Cheap to clone, for a task to carry.
#[derive(Clone)]
pub(super) struct Hooks {
    pub(super) kind: Kind,
    pub(super) server: WeakEntity<Server>,
    pub(super) checkout: WeakEntity<Checkout>,
    pub(super) path: Arc<Path>,
    /// The completions the server last listed for a word, filtered as it's typed on.
    pub(super) completions: Rc<RefCell<Completions>>,
}

/// The file as the server knows it, with what's needed to ask about it and convert the
/// answer: taken while the server's running and has been given the text.
pub(super) struct Asked {
    pub(super) client: Client,
    pub(super) document: TextDocumentIdentifier,
    shown: Shown,
    pub(super) encoding: Encoding,
}

impl Hooks {
    /// The file as the server has it, if the server's running and has the text `rope`:
    /// another version than the editor's, and positions would mean nothing.
    pub(super) fn ask(&self, rope: &Rope, cx: &App) -> Option<Asked> {
        self.as_shown(cx).filter(|asked| asked.len() == rope.len())
    }

    /// The file as the server has it, whatever text that is: for a request about a row
    /// of a list, which carries no position.
    pub(super) fn as_shown(&self, cx: &App) -> Option<Asked> {
        let server = self.server.upgrade()?;
        let server = server.read(cx);
        let client = server.client.clone()?;
        let open = server.documents.get(&*self.path).filter(|open| open.sent)?;
        Some(Asked {
            client,
            document: TextDocumentIdentifier::new(open.lsp.uri().clone()),
            shown: open.shown.clone()?,
            encoding: server.encoding,
        })
    }

    /// What the server said it can do, once it's initialized.
    pub(super) fn capabilities(&self, cx: &App) -> Option<Arc<ServerCapabilities>> {
        let server = self.server.upgrade()?;
        server.read(cx).client.as_ref()?.capabilities()
    }
}

/// Whether a server's say on a provider is yes: `true`, or its options.
pub(super) fn provided<T>(provider: Option<&OneOf<bool, T>>) -> bool {
    matches!(provider, Some(OneOf::Left(true) | OneOf::Right(_)))
}

/// Asks `client` `R`, again while the server says the workspace changed under it, and logs
/// how long the answer took. A failure is logged and taken as no answer.
pub(super) async fn ask<R: Request>(client: &Client, params: &R::Params) -> Option<R::Result> {
    let asked = Instant::now();
    let answer = client.request_retrying::<R>(params).await;
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

impl Asked {
    /// How long the text the server has is, in bytes.
    fn len(&self) -> usize {
        self.shown.text.as_str().len()
    }

    /// The editor's version of the text the server has.
    pub(super) fn version(&self) -> u64 {
        self.shown.version
    }

    /// The byte of the text a position of the server's stands for.
    pub(super) fn offset(&self, position: lsp_types::Position) -> usize {
        self.shown.text.offset(position, self.encoding)
    }

    /// The byte `offset` of the text as the server knows it.
    pub(super) fn at(&self, offset: usize) -> TextDocumentPositionParams {
        TextDocumentPositionParams {
            text_document: self.document.clone(),
            position: self.shown.text.position(offset, self.encoding),
        }
    }

    /// Asks the server `R`, as [`ask`] does.
    pub(super) async fn ask<R: Request>(&self, params: &R::Params) -> Option<R::Result> {
        ask::<R>(&self.client, params).await
    }

    /// The bytes `range` of the text, as the server knows it.
    pub(super) fn slice(&self, range: Range<usize>) -> Option<&str> {
        self.shown.text.as_str().get(range)
    }

    /// A range of bytes of the text, as the server knows it.
    pub(super) fn for_server(&self, range: Range<usize>) -> lsp_types::Range {
        self.shown.text.range(range, self.encoding)
    }

    /// The bytes of the text a range of the server's stands for.
    pub(super) fn bytes(&self, range: lsp_types::Range) -> Range<usize> {
        self.shown.text.offsets(range, self.encoding)
    }

    /// A range in this file, from the server's encoding to the editor's positions, whose
    /// columns count characters.
    pub(super) fn for_editor(&self, range: lsp_types::Range) -> lsp_types::Range {
        self.shown.text.range(self.bytes(range), Encoding::Utf32)
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
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(Ok(Vec::new()));
        };
        let path = self.path.clone();
        // Dropped by the editor once it asks again, which cancels the request.
        cx.background_spawn(async move {
            let params = GotoDefinitionParams {
                text_document_position_params: asked.at(offset),
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
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(Ok(None));
        };
        cx.background_spawn(async move {
            let params = HoverParams {
                text_document_position_params: asked.at(offset),
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
