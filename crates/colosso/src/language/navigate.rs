//! A definition in another file, as the server names it, turned into a place in that file
//! for the Files panel: its range, in the server's encoding, converted to bytes of the file's
//! text once that's read.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use colosso_lsp::lsp_types::Range;
use colosso_lsp::{Encoding, LineIndex, uri};
use gpui_kit::component::input::ShowDocumentHandler;
use gpui_kit::{App, Entity, WeakEntity};

use super::server::Server;
use crate::checkout::{Checkout, Content, Document};
use crate::files::{At, FileEditor, Place};

/// What the editor calls to show a definition: one in another file is shown in the viewer,
/// as the place `view` says it leads to; one in this file the editor goes to itself.
pub(super) fn show_document(
    server: WeakEntity<Server>,
    checkout: WeakEntity<Checkout>,
    path: Arc<Path>,
    view: WeakEntity<FileEditor>,
) -> ShowDocumentHandler {
    Rc::new(move |params, _, cx| {
        let Some(target) = uri::to_path(&params.uri) else {
            return false;
        };
        let (Some(selection), Some(running)) = (params.selection, server.upgrade()) else {
            return false;
        };
        if target.as_path() == &*path {
            return false;
        }
        let encoding = running.read(cx).encoding;
        let (server, checkout, view) = (server.clone(), checkout.clone(), view.clone());
        // The editor is mid-update; the file is shown once it's done, and the text read.
        cx.defer(move |cx| {
            let Some(checkout) = checkout.upgrade() else {
                return;
            };
            let document = checkout.update(cx, |checkout, cx| checkout.document(&target, cx));
            let read = document.update(cx, |document, _| document.read());
            cx.spawn(async move |cx| {
                read.await;
                cx.update(|cx| {
                    let bytes = bytes(&document, &server, selection, encoding, cx);
                    let place = Place {
                        path: target,
                        at: bytes.map(At::Bytes),
                    };
                    // The document's held until the place is shown, so it's read just once.
                    view.update(cx, |_, cx| cx.emit(place)).ok();
                    drop(document);
                });
            })
            .detach();
        });
        true
    })
}

/// The bytes of `document`'s text the server's `range` stands for, once it's read as text:
/// through the lines the server's copy of it has indexed, when it has the file open, and
/// else through the text's own.
fn bytes(
    document: &Entity<Document>,
    server: &WeakEntity<Server>,
    range: Range,
    encoding: Encoding,
    cx: &App,
) -> Option<std::ops::Range<usize>> {
    let document = document.read(cx);
    let Content::Text(text) = document.content() else {
        return None;
    };
    let shown = (server.upgrade()).and_then(|server| server.read(cx).shown_text(document.path()));
    Some(match shown {
        Some(shown) => shown.offsets(range, encoding),
        None => LineIndex::new(text).offsets(text, range, encoding),
    })
}
