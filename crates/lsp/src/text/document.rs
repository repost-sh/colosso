//! An open document's version, and what's sent to keep the server's copy in step: the whole
//! text when it's opened or replaced, borrowed rather than copied, and edits in between.

use lsp_types::{
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextEdit, Uri,
    VersionedTextDocumentIdentifier,
};
use serde::Serialize;

use crate::outgoing::Params;

/// A document open on a server: which one, and the version the server has. Every change
/// counts the version up, so the server can say which version an answer is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    uri: Uri,
    version: i32,
}

/// `textDocument/didOpen`'s params, borrowing the text.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened<'a> {
    text_document: OpenedText<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenedText<'a> {
    uri: &'a Uri,
    language_id: &'a str,
    version: i32,
    text: &'a str,
}

impl Params<DidOpenTextDocumentParams> for Opened<'_> {}

/// `textDocument/didChange`'s params for a whole new text, borrowing it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replaced<'a> {
    text_document: Versioned<'a>,
    content_changes: [WholeText<'a>; 1],
}

#[derive(Serialize)]
struct Versioned<'a> {
    uri: &'a Uri,
    version: i32,
}

#[derive(Serialize)]
struct WholeText<'a> {
    text: &'a str,
}

impl Params<DidChangeTextDocumentParams> for Replaced<'_> {}

impl Document {
    /// A document at version 0, not yet open on the server.
    pub fn new(uri: Uri) -> Document {
        Document { uri, version: 0 }
    }

    pub fn uri(&self) -> &Uri {
        &self.uri
    }

    /// The version the server has.
    pub fn version(&self) -> i32 {
        self.version
    }

    /// What opens `text` on the server, at the current version.
    pub fn open<'a>(&'a self, language_id: &'a str, text: &'a str) -> Opened<'a> {
        Opened {
            text_document: OpenedText {
                uri: &self.uri,
                language_id,
                version: self.version,
                text,
            },
        }
    }

    /// The change that makes `edits` on the server, as the next version. The server makes
    /// them in order, each range in the text as the edits before it left it: as
    /// [`IndexedText::edit`](super::IndexedText::edit) returns them, or an editor's own rope
    /// reports them.
    pub fn change(
        &mut self,
        edits: impl IntoIterator<Item = TextEdit>,
    ) -> DidChangeTextDocumentParams {
        self.version += 1;
        DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(self.uri.clone(), self.version),
            content_changes: edits
                .into_iter()
                .map(|edit| TextDocumentContentChangeEvent {
                    range: Some(edit.range),
                    range_length: None,
                    text: edit.new_text,
                })
                .collect(),
        }
    }

    /// The change that replaces the whole text, as the next version: for servers that only
    /// take whole texts, or an edit too big to be worth working out.
    pub fn replace<'a>(&'a mut self, text: &'a str) -> Replaced<'a> {
        self.version += 1;
        Replaced {
            text_document: Versioned {
                uri: &self.uri,
                version: self.version,
            },
            content_changes: [WholeText { text }],
        }
    }

    /// What to send when the document is saved, with its text if the server asked for it.
    pub fn save(&self, text: Option<&str>) -> DidSaveTextDocumentParams {
        DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier::new(self.uri.clone()),
            text: text.map(str::to_owned),
        }
    }
}
