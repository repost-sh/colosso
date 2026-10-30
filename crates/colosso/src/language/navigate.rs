//! A definition in another file, as the server names it, turned into a place in that file
//! for the Files panel: its range in the server's encoding, made bytes of the file's text
//! once the file's shown.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use colosso_lsp::uri;
use gpui_kit::WeakEntity;
use gpui_kit::component::input::ShowDocumentHandler;

use super::server::Server;
use crate::files::{At, FileEditor, Place};

/// What the editor calls to show a definition: one in another file is shown in the viewer,
/// as the place `view` says it leads to; one in this file the editor goes to itself.
pub(super) fn show_document(
    server: WeakEntity<Server>,
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
        let place = Place {
            path: target,
            at: Some(At::Server(selection, running.read(cx).encoding)),
        };
        let view = view.clone();
        // The editor is mid-update; the file is shown once it's done.
        cx.defer(move |cx| {
            view.update(cx, |_, cx| cx.emit(place)).ok();
        });
        true
    })
}
