//! The files open on a server: opened with their text once it's read, sent whole again each
//! time the checkout's document reads it again, and closed once the last view of the document
//! lets go of it.

use std::path::Path;
use std::sync::Arc;

use colosso_lsp::lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
};
use colosso_lsp::lsp_types::{DidCloseTextDocumentParams, TextDocumentIdentifier};
use colosso_lsp::{Client, LineIndex, uri};
use gpui_kit::{Context, Entity, EntityId, SharedString, Subscription};

use super::Kind;
use super::server::{Activity, Server};
use crate::checkout::{Content, Document};

/// A file open on a server.
pub(in crate::language) struct Open {
    /// The checkout's document for it, which the views showing it hold.
    document: EntityId,
    /// Its name on the server, and the version the server has.
    pub(super) lsp: colosso_lsp::Document,
    /// Its text as last read, once it's read.
    pub(super) shown: Option<Shown>,
    /// Whether the running server has it open.
    pub(super) sent: bool,
    _changed: Subscription,
    _released: Subscription,
}

/// A file's text as the server has it, with its lines indexed, so positions convert both
/// ways in the encoding the server chose.
#[derive(Clone)]
pub(in crate::language) struct Shown {
    pub(super) text: SharedString,
    pub(super) lines: Arc<LineIndex>,
    /// The checkout's document's version this is.
    pub(super) version: u64,
}

impl Server {
    /// Opens `document` on the server, now if its text is read and the server's running,
    /// or else once both are.
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
        let _changed = cx.observe(document, |this, document, cx| this.read(&document, cx));
        let released = path.clone();
        let _released = cx.observe_release(document, move |this, _, cx| {
            this.close(&released, id, cx);
        });
        let open = Open {
            document: id,
            lsp: colosso_lsp::Document::new(uri),
            shown: None,
            sent: false,
            _changed,
            _released,
        };
        let was_idle = self.documents.is_empty();
        self.documents.insert(path, open);
        if was_idle {
            cx.emit(Activity::Busy);
        }
        self.read(document, cx);
    }

    /// Takes the document's text, once it's read and each time it's read again, and sends it.
    fn read(&mut self, document: &Entity<Document>, cx: &mut Context<Self>) {
        let document = document.read(cx);
        let Content::Text(text) = document.content() else {
            return;
        };
        let Some(open) = self.documents.get_mut(document.path()) else {
            return;
        };
        let version = document.version();
        if open
            .shown
            .as_ref()
            .is_some_and(|shown| shown.version == version)
        {
            return;
        }
        open.shown = Some(Shown {
            text: text.clone(),
            lines: Arc::new(LineIndex::new(text)),
            version,
        });
        if let Some(client) = &self.client {
            send(client, self.kind, open);
        }
        // The editors showing it convert its diagnostics again.
        cx.notify();
    }

    /// The last view of the document at `path` let go of it.
    fn close(&mut self, path: &Arc<Path>, id: EntityId, cx: &mut Context<Self>) {
        // A document read again since replaced this one under the same path.
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

    /// Opens every file read so far on a server that has just been initialized.
    pub(super) fn send_all(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        for open in self.documents.values_mut() {
            send(client, self.kind, open);
        }
    }
}

/// Sends the file's text: opening it, or replacing the text the server has.
fn send(client: &Client, kind: Kind, open: &mut Open) {
    let Some(shown) = &open.shown else {
        return;
    };
    let sent = if open.sent {
        client.notify::<DidChangeTextDocument>(&open.lsp.replace(&shown.text))
    } else {
        client.notify::<DidOpenTextDocument>(&open.lsp.open(kind.language_id(), &shown.text))
    };
    match sent {
        Ok(()) => open.sent = true,
        Err(error) => log::debug!("couldn't send {}: {error}", open.lsp.uri().as_str()),
    }
}
