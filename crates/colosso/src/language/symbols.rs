//! The symbols a server names, for the palettes that go to one: the file's, nested ones
//! after what they're in (`textDocument/documentSymbol`); and the checkout's whose name
//! has what's typed in it, as each of its servers says (`workspace/symbol`). Each comes
//! with its place as the server names it, for the Files panel to find in the file's text.

use std::path::Path;
use std::sync::Arc;

use colosso_lsp::lsp_types::request::{DocumentSymbolRequest, WorkspaceSymbolRequest};
use colosso_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, OneOf, PartialResultParams,
    SymbolInformation, SymbolKind, WorkDoneProgressParams, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};
use colosso_lsp::{Encoding, uri};
use futures::future::join_all;
use gpui_kit::component::input::Rope;
use gpui_kit::{App, AppContext as _, Entity, SharedString, Task};

use super::providers::{Hooks, ask, provided};
use super::registry::Languages;
use super::server::Server;
use crate::files::At;

/// A symbol a server named.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: SharedString,
    pub kind: SymbolKind,
    /// What it's in: the symbol it's nested in, or the container the server names.
    pub container: Option<SharedString>,
    /// More about it, when the server says: a function's signature.
    pub detail: Option<SharedString>,
    /// The file it's in.
    pub path: Arc<Path>,
    /// Where in the file, as the server says; nothing when it named only the file.
    pub at: Option<At>,
}

impl Hooks {
    /// The file's symbols, in the server's order, nested ones after what they're in;
    /// none while the server isn't running, or doesn't list them.
    pub(super) fn document_symbols(&self, rope: &Rope, cx: &App) -> Task<Vec<Symbol>> {
        let Some(asked) = self.ask(rope, cx) else {
            return Task::ready(Vec::new());
        };
        let lists = (self.capabilities(cx))
            .is_some_and(|capabilities| provided(capabilities.document_symbol_provider.as_ref()));
        if !lists {
            log::info!("{} doesn't list a file's symbols", self.kind.name());
            return Task::ready(Vec::new());
        }
        let path = self.path.clone();
        cx.background_spawn(async move {
            let params = DocumentSymbolParams {
                text_document: asked.document.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };
            let encoding = asked.encoding;
            match asked.ask::<DocumentSymbolRequest>(&params).await.flatten() {
                Some(DocumentSymbolResponse::Nested(nested)) => {
                    let mut symbols = Vec::new();
                    flatten(nested, None, encoding, &path, &mut symbols);
                    symbols
                }
                Some(DocumentSymbolResponse::Flat(flat)) => (flat.into_iter())
                    .map(|info| informed(info, &path, encoding))
                    .collect(),
                None => Vec::new(),
            }
        })
    }
}

/// `nested` and what's in them, each after what it's in and named for it.
fn flatten(
    nested: Vec<DocumentSymbol>,
    container: Option<&SharedString>,
    encoding: Encoding,
    path: &Arc<Path>,
    out: &mut Vec<Symbol>,
) {
    for DocumentSymbol { name, kind, detail, selection_range, children, .. } in nested {
        let at = Some(At::Server(selection_range, encoding));
        let symbol = Symbol {
            detail: detail.map(Into::into),
            ..named(name, kind, container.cloned(), path, at)
        };
        let name = symbol.name.clone();
        out.push(symbol);
        flatten(children.unwrap_or_default(), Some(&name), encoding, path, out);
    }
}

/// A symbol called `name`, in `container`, `at` a place of `path`, with nothing more said
/// of it.
fn named(
    name: String,
    kind: SymbolKind,
    container: Option<SharedString>,
    path: &Arc<Path>,
    at: Option<At>,
) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        container,
        detail: None,
        path: path.clone(),
        at,
    }
}

/// The symbol `info` names, at its range of `path`.
fn informed(info: SymbolInformation, path: &Arc<Path>, encoding: Encoding) -> Symbol {
    let at = Some(At::Server(info.location.range, encoding));
    named(info.name, info.kind, info.container_name.map(Into::into), path, at)
}

/// The servers of the checkout at `root`, started or not.
fn servers_at(root: &Path, cx: &App) -> Vec<Entity<Server>> {
    let Some(Languages(registry)) = cx.try_global::<Languages>() else {
        return Vec::new();
    };
    registry.read(cx).servers_at(root)
}

/// Whether the checkout at `root` has a server to ask for its symbols: one starts with the
/// first of its files shown.
pub fn serves(root: &Path, cx: &App) -> bool {
    !servers_at(root, cx).is_empty()
}

/// The symbols of the checkout at `root` with `query` in their name, as each of its
/// servers that's running and finds symbols says, the servers asked together.
pub fn workspace_symbols(root: &Path, query: String, cx: &App) -> Task<Vec<Symbol>> {
    let params = WorkspaceSymbolParams {
        query,
        partial_result_params: PartialResultParams::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let answers: Vec<Task<Vec<Symbol>>> = (servers_at(root, cx).into_iter())
        .filter_map(|server| {
            let server = server.read(cx);
            let client = server.client.clone()?;
            let finds = provided(client.capabilities()?.workspace_symbol_provider.as_ref());
            finds.then_some((client, server.encoding))
        })
        .map(|(client, encoding)| {
            let params = params.clone();
            cx.background_spawn(async move {
                let answer = ask::<WorkspaceSymbolRequest>(&client, &params).await.flatten();
                answer.map_or_else(Vec::new, |answer| found(answer, encoding))
            })
        })
        .collect();
    cx.background_spawn(async move { join_all(answers).await.concat() })
}

/// The symbols a server's `answer` names, each at its place in the server's `encoding`.
fn found(answer: WorkspaceSymbolResponse, encoding: Encoding) -> Vec<Symbol> {
    match answer {
        WorkspaceSymbolResponse::Flat(flat) => (flat.into_iter())
            .filter_map(|info| {
                let path = uri::to_path(&info.location.uri)?;
                Some(informed(info, &path.into(), encoding))
            })
            .collect(),
        WorkspaceSymbolResponse::Nested(nested) => (nested.into_iter())
            .filter_map(|symbol| {
                let (uri, at) = match symbol.location {
                    OneOf::Left(location) => (location.uri, Some(At::Server(location.range, encoding))),
                    // Only the file. The client doesn't offer to ask for the range after, so
                    // no server should say so little.
                    OneOf::Right(location) => (location.uri, None),
                };
                let path = uri::to_path(&uri)?;
                let container = symbol.container_name.map(Into::into);
                Some(named(symbol.name, symbol.kind, container, &path.into(), at))
            })
            .collect(),
    }
}
