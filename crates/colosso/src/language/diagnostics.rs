//! What a server published for each file: converted to bytes of the text the server had as
//! it comes, kept by path so a file shown later shows it at once, and put into the editor
//! showing the file with each range brought up to the editor's text through its edits.
//! Once in, they follow the edits made to the text until the server publishes again.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_lsp::lsp_types::{self, PublishDiagnosticsParams};
use colosso_lsp::uri;
use gpui_kit::Context;
use gpui_kit::base::input::Diagnostic;
use gpui_kit::component::input::EditorState;

use super::server::Server;

/// What a server published for a file, as bytes of the editor's text at a version of it.
#[derive(Default)]
pub(in crate::language) struct Converted {
    /// The version of the editor's text the bytes are in.
    version: u64,
    items: Vec<(Range<usize>, lsp_types::Diagnostic)>,
}

impl Converted {
    /// The diagnostics touching `range` of the text they're in, as the server gave them,
    /// to be handed back when asking what could be done about them.
    pub(super) fn within(&self, range: &Range<usize>) -> Vec<lsp_types::Diagnostic> {
        (self.items.iter())
            .filter(|(bytes, _)| bytes.start <= range.end && range.start <= bytes.end)
            .map(|(_, diagnostic)| diagnostic.clone())
            .collect()
    }
}

/// The diagnostics published for each file, each with the count at which it was published,
/// so an editor can tell whether its copy is out of date.
#[derive(Default)]
pub(in crate::language) struct Published {
    by_path: HashMap<PathBuf, (u64, Arc<Converted>)>,
    count: u64,
}

impl Published {
    /// What was published for `path`, and when, or nothing and the start.
    pub(super) fn get(&self, path: &Path) -> (u64, Arc<Converted>) {
        (self.by_path.get(path).cloned()).unwrap_or_default()
    }

    fn set(&mut self, path: PathBuf, converted: Converted) {
        self.count += 1;
        self.by_path.insert(path, (self.count, Arc::new(converted)));
    }

    /// Drops what was published for `path`: the file's closed, and the text they're in gone.
    pub(super) fn forget(&mut self, path: &Path) {
        self.by_path.remove(path);
    }

    /// Clears them all: the server that published them has gone.
    pub(super) fn clear(&mut self) {
        let paths: Vec<PathBuf> = self.by_path.keys().cloned().collect();
        for path in paths {
            self.set(path, Converted::default());
        }
    }
}

impl Server {
    /// Keeps what was published for a file, as bytes of the text the server has, unless
    /// it's for a version of the file other than that one.
    pub(super) fn published(&mut self, params: &PublishDiagnosticsParams, cx: &mut Context<Self>) {
        let Some(path) = uri::to_path(&params.uri) else {
            return;
        };
        let Some(open) = self.documents.get(path.as_path()).filter(|open| open.sent) else {
            return;
        };
        if let Some(version) = params.version
            && open.lsp.version() != version
        {
            log::debug!(
                "dropped diagnostics for version {version} of {}, which is at {}",
                path.display(),
                open.lsp.version()
            );
            return;
        }
        let Some(shown) = &open.shown else {
            return;
        };
        let items = (params.diagnostics.iter())
            .map(|diagnostic| {
                let bytes = shown.text.offsets(diagnostic.range, self.encoding);
                (bytes, diagnostic.clone())
            })
            .collect();
        let converted = Converted {
            version: shown.version,
            items,
        };
        self.published.set(path, converted);
        cx.notify();
    }
}

/// Puts `converted` into `editor`, each range brought up to its text; one the editor's
/// edits no longer reach back to is left out.
pub(super) fn show(editor: &mut EditorState, converted: &Converted, cx: &mut Context<EditorState>) {
    let mut items: Vec<_> = (converted.items.iter())
        .filter_map(|(bytes, diagnostic)| {
            let bytes = editor.translate_range(bytes.clone(), converted.version)?;
            Some((bytes, Diagnostic::from(diagnostic.clone())))
        })
        .collect();
    // The set takes them in the order of their ranges.
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
