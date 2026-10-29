//! What a server published for each file: kept by path, so a file shown later shows it at
//! once, and put into the editor showing the file with each range converted from the
//! server's encoding to the editor's bytes. Once in, they follow the edits made to the
//! text until the server publishes again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_lsp::lsp_types::{self, PublishDiagnosticsParams};
use colosso_lsp::{Encoding, uri};
use gpui_kit::Context;
use gpui_kit::base::input::Diagnostic;
use gpui_kit::component::input::EditorState;

use super::server::Server;
use super::sync::Shown;

/// The diagnostics published for each file, each with the count at which it was published,
/// so an editor can tell whether its copy is out of date.
#[derive(Default)]
pub(in crate::language) struct Published {
    by_path: HashMap<PathBuf, (u64, Arc<[lsp_types::Diagnostic]>)>,
    count: u64,
}

impl Published {
    /// What was published for `path`, and when, or nothing and the start.
    pub(super) fn get(&self, path: &Path) -> (u64, Arc<[lsp_types::Diagnostic]>) {
        (self.by_path.get(path).cloned()).unwrap_or_else(|| (0, Arc::from([])))
    }

    fn set(&mut self, path: PathBuf, diagnostics: Arc<[lsp_types::Diagnostic]>) {
        self.count += 1;
        self.by_path.insert(path, (self.count, diagnostics));
    }

    /// Clears them all: the server that published them has gone.
    pub(super) fn clear(&mut self) {
        let paths: Vec<PathBuf> = self.by_path.keys().cloned().collect();
        for path in paths {
            self.set(path, Arc::from([]));
        }
    }
}

impl Server {
    /// Keeps what was published for a file, unless it's for a version of it other than the
    /// one the server has.
    pub(super) fn published(&mut self, params: &PublishDiagnosticsParams, cx: &mut Context<Self>) {
        let Some(path) = uri::to_path(&params.uri) else {
            return;
        };
        let open = self.documents.get(path.as_path()).filter(|open| open.sent);
        if let (Some(version), Some(open)) = (params.version, open)
            && open.lsp.version() != version
        {
            log::debug!(
                "dropped diagnostics for version {version} of {}, which is at {}",
                path.display(),
                open.lsp.version()
            );
            return;
        }
        self.published.set(path, params.diagnostics.clone().into());
        cx.notify();
    }
}

/// Puts `diagnostics`, published for the text `shown`, into `editor`, which shows that text.
pub(super) fn show(
    editor: &mut EditorState,
    diagnostics: &[lsp_types::Diagnostic],
    shown: &Shown,
    encoding: Encoding,
    cx: &mut Context<EditorState>,
) {
    let mut items: Vec<_> = (diagnostics.iter())
        .map(|diagnostic| {
            let bytes = (shown.lines).offsets(&shown.text, diagnostic.range, encoding);
            (bytes, Diagnostic::from(diagnostic.clone()))
        })
        .collect();
    // The set seeks by where each starts, so it takes them in that order.
    items.sort_by_key(|(bytes, _)| bytes.start);
    let Some(set) = editor.diagnostics_mut() else {
        return;
    };
    set.clear();
    for (bytes, item) in items {
        set.push_range(bytes, item);
    }
    cx.notify();
}
