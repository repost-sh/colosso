//! The files open on a server: opened with their text once a view shows it, sent the edits
//! the view makes, told when they're saved, and closed once the last view of the document
//! lets go of it.

use std::path::Path;
use std::sync::Arc;

use colosso_lsp::lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
};
use colosso_lsp::lsp_types::{DidCloseTextDocumentParams, TextDocumentIdentifier};
use colosso_lsp::{Client, IndexedText, uri};
use gpui_kit::component::input::EditorState;
use gpui_kit::{Context, Entity, EntityId, Subscription};

use super::server::{Activity, Server};
use crate::checkout::{Document, DocumentEvent};

/// Past this many edits at once, the text goes to the server whole instead: each edit
/// costs the server's copy a pass over the text.
const MOST_EDITS: usize = 32;

/// A file open on a server.
pub(in crate::language) struct Open {
    /// The checkout's document for it, which the views showing it hold.
    document: EntityId,
    /// Its name on the server, and the version the server has.
    pub(super) lsp: colosso_lsp::Document,
    /// The language it's opened as.
    language_id: &'static str,
    /// Its text as the server has it, once a view gave it.
    pub(super) shown: Option<Shown>,
    /// Whether the running server has it open.
    pub(super) sent: bool,
    _released: Subscription,
    _saved: Subscription,
}

/// A file's text as the server has it, its lines indexed so positions convert both ways in
/// the encoding the server chose; and the version of the editor's text it is.
#[derive(Clone)]
pub(in crate::language) struct Shown {
    pub(super) text: Arc<IndexedText>,
    pub(super) version: u64,
}

impl Shown {
    fn new(editor: &EditorState) -> Shown {
        Shown {
            text: Arc::new(IndexedText::new(editor.text().to_string())),
            version: editor.version(),
        }
    }
}

impl Server {
    /// Opens `document` on the server once a view gives its text, and keeps it open until
    /// the last view lets go of the document.
    pub(super) fn open(&mut self, document: &Entity<Document>, cx: &mut Context<Self>) {
        let id = document.entity_id();
        let path = document.read(cx).path().clone();
        if self
            .documents
            .get(&path)
            .is_some_and(|open| open.document == id)
        {
            return;
        }
        let Some(uri) = uri::from_path(&path) else {
            log::debug!("{} has no URI to open it by", path.display());
            return;
        };
        let released = path.clone();
        let _released = cx.observe_release(document, move |this, _, cx| {
            this.close(&released, id, cx);
        });
        let saved = path.clone();
        let _saved = cx.subscribe(document, move |this, _, event, _| {
            if let DocumentEvent::Saved = event {
                this.saved(&saved);
            }
        });
        let open = Open {
            document: id,
            lsp: colosso_lsp::Document::new(uri),
            language_id: super::language_id(&path),
            shown: None,
            sent: false,
            _released,
            _saved,
        };
        let was_idle = self.documents.is_empty();
        self.documents.insert(path, open);
        if was_idle {
            cx.emit(Activity::Busy);
        }
    }

    /// `editor` shows the file at `path`: its text goes to the server whole, opening the
    /// file or replacing what it had.
    pub(super) fn show(&mut self, path: &Path, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(open) = self.documents.get_mut(path) else {
            return;
        };
        open.shown = Some(Shown::new(editor.read(cx)));
        if let Some(client) = &self.client {
            send(client, open);
        }
        cx.notify();
    }

    /// The editor showing the file at `path` edited its text: the server gets the edits
    /// since it last heard, or the whole text again if the editor no longer has them, or
    /// there are too many to be worth it.
    pub(super) fn changed(&mut self, path: &Path, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(open) = self.documents.get_mut(path) else {
            return;
        };
        let Some(shown) = &mut open.shown else {
            return;
        };
        let state = editor.read(cx);
        let version = state.version();
        if shown.version == version {
            return;
        }
        let encoding = self.encoding;
        let edits = state.edits_since(shown.version).and_then(|edits| {
            let edits: Vec<_> = edits.collect();
            if edits.len() > MOST_EDITS {
                return None;
            }
            let text = Arc::make_mut(&mut shown.text);
            (edits.into_iter())
                .map(|edit| text.edit(edit.old.clone(), &edit.new_text, encoding))
                .collect::<Result<Vec<_>, _>>()
                .ok()
        });
        let Some(edits) = edits else {
            self.show(path, editor, cx);
            return;
        };
        shown.version = version;
        if let Some(client) = self.client.as_ref().filter(|_| open.sent)
            && let Err(error) = client.notify::<DidChangeTextDocument>(&open.lsp.change(edits))
        {
            log::debug!("couldn't send edits to {}: {error}", path.display());
        }
    }

    /// The file at `path` was saved, with the text the server has.
    fn saved(&self, path: &Path) {
        let Some(open) = self.documents.get(path).filter(|open| open.sent) else {
            return;
        };
        if let Some(client) = &self.client
            && let Err(error) = client.notify::<DidSaveTextDocument>(&open.lsp.save(None))
        {
            log::debug!("couldn't say {} was saved: {error}", path.display());
        }
    }

    /// The last view of the document `id` at `path` let go of it: what the server published
    /// for the file goes too, as it's in a text no editor has.
    fn close(&mut self, path: &Arc<Path>, id: EntityId, cx: &mut Context<Self>) {
        // A document opened again since replaced this one under the same path.
        if self
            .documents
            .get(path)
            .is_none_or(|open| open.document != id)
        {
            return;
        }
        let Some(open) = self.documents.remove(path) else {
            return;
        };
        self.published.forget(path);
        if let Some(client) = self.client.as_ref().filter(|_| open.sent) {
            let params = DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier::new(open.lsp.uri().clone()),
            };
            if let Err(error) = client.notify::<DidCloseTextDocument>(&params) {
                log::debug!("couldn't close {}: {error}", path.display());
            }
        }
        if self.documents.is_empty() {
            cx.emit(Activity::Idle);
        }
    }

    /// Opens every file shown so far on a server that has just been initialized.
    pub(super) fn send_all(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        for open in self.documents.values_mut() {
            send(client, open);
        }
    }
}

/// Sends the file's text: opening it, or replacing the text the server has.
fn send(client: &Client, open: &mut Open) {
    let Some(shown) = &open.shown else {
        return;
    };
    let text = shown.text.as_str();
    let sent = if open.sent {
        client.notify::<DidChangeTextDocument>(&open.lsp.replace(text))
    } else {
        client.notify::<DidOpenTextDocument>(&open.lsp.open(open.language_id, text))
    };
    match sent {
        Ok(()) => open.sent = true,
        Err(error) => log::debug!("couldn't send {}: {error}", open.lsp.uri().as_str()),
    }
}
