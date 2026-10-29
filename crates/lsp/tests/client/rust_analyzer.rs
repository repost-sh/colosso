//! The client against a real rust-analyzer, the managed one, on a crate of two modules: what
//! an editor asks for most, and how long each answer takes.
//!
//! Indexing done, rust-analyzer can still reload its workspace once more, and answers the
//! requests it was working on then with `ContentModified`. The test waits until the server
//! says it's ready, and asks as an editor does, again once it's ready after a reload.
//!
//! It downloads rust-analyzer the first time, so it runs only when asked:
//! `COLOSSO_LSP_IT=1 COLOSSO_DATA_DIR=$PWD/.superpowers/data cargo test -p colosso-lsp -- --ignored`

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use colosso_lsp::lsp_types::notification::{
    DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, PublishDiagnostics,
};
use colosso_lsp::lsp_types::request::{Completion, GotoDefinition, HoverRequest, Request};
use colosso_lsp::lsp_types::{
    CompletionContext, CompletionParams, CompletionResponse, CompletionTriggerKind,
    DiagnosticSeverity, GotoDefinitionParams, GotoDefinitionResponse, HoverContents, HoverParams,
    PartialResultParams, Position, PublishDiagnosticsParams, TextDocumentIdentifier,
    TextDocumentPositionParams, WorkDoneProgressParams,
};
use colosso_lsp::servers::{Curl, RUST_ANALYZER};
use colosso_lsp::{Client, Document, Encoding, IndexedText, LineIndex, Notifications, uri};
use futures::StreamExt;

use crate::common::wait;

const MANIFEST: &str = "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
    # A workspace of its own, not the checkout's it sits in.\n[workspace]\n";
const LIB: &str = "pub mod a;\npub mod b;\n";
// The crab before the call puts its column at different numbers in UTF-8 and UTF-16.
const A: &str = "use crate::b::make_counter;\n\npub fn total() -> i32 {\n    \
    let counter = /* 🦀 */ make_counter(2);\n    counter.doubled()\n}\n\n\
    pub fn wrong() -> u32 {\n    \"not a number\"\n}\n";
const B: &str = "/// Counts, for the test.\npub struct Counter {\n    value: i32,\n}\n\n\
    /// A counter starting at `value`.\npub fn make_counter(value: i32) -> Counter {\n    \
    Counter { value }\n}\n\nimpl Counter {\n    /// Twice the value.\n    \
    pub fn doubled(&self) -> i32 {\n        self.value * 2\n    }\n}\n";
const MORE: &str =
    "\npub fn more() -> i32 {\n    let counter = make_counter(3);\n    counter.\n}\n";

#[test]
#[ignore = "downloads and runs rust-analyzer; see this file's header"]
fn rust_analyzer_answers_what_an_editor_asks() {
    if std::env::var("COLOSSO_LSP_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set COLOSSO_LSP_IT=1 to run against rust-analyzer");
        return;
    }
    let data = std::env::var_os("COLOSSO_DATA_DIR")
        .map(PathBuf::from)
        .expect("COLOSSO_DATA_DIR names a folder in the checkout to install into");
    let binary = RUST_ANALYZER.install(&data, &Curl).unwrap();
    let root = fixture();
    let client = Client::start(&RUST_ANALYZER.launch(binary, &root)).unwrap();
    let mut diagnostics = client.subscribe::<PublishDiagnostics>();

    let started = Instant::now();
    wait(60, client.initialize(&root)).unwrap();
    let mut latencies = vec![("initialize", started.elapsed())];
    let a_uri = uri::from_path(&root.join("src/a.rs")).unwrap();
    let mut document = Document::new(a_uri);
    client
        .notify::<DidOpenTextDocument>(&document.open("rust", A))
        .unwrap();
    wait(300, client.ready()).unwrap();
    latencies.push(("ready", started.elapsed()));

    let editor = Editor {
        client: &client,
        encoding: client.encoding(),
        root: &root,
    };
    latencies.push(("definition", editor.definition(&document)));
    latencies.push(("hover", editor.hover(&document)));
    let saved = editor.diagnostics(&document, &mut diagnostics);
    latencies.push(("diagnostics after save", saved));
    latencies.push(("completion", editor.completion(&mut document)));

    let started = Instant::now();
    let exit = wait(60, client.shutdown(Duration::from_secs(10)));
    latencies.push(("shutdown", started.elapsed()));
    assert!(!exit.crashed && !exit.killed, "{exit:?}");
    eprintln!("rust-analyzer, {:?} positions:", editor.encoding);
    for (step, took) in latencies {
        eprintln!("  {step:<24}{:>9.1} ms", took.as_secs_f64() * 1000.0);
    }
}

/// What an editor asks, each step returning how long it took to be answered.
struct Editor<'a> {
    client: &'a Client,
    encoding: Encoding,
    root: &'a Path,
}

impl Editor<'_> {
    /// From the call in a.rs to the function in b.rs.
    fn definition(&self, document: &Document) -> Duration {
        let call = A.find("make_counter(2)").unwrap();
        let params = GotoDefinitionParams {
            text_document_position_params: at(document, self.position(A, call)),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let (definition, took) = self.ask::<GotoDefinition>(&params);
        let Some(GotoDefinitionResponse::Link(links)) = definition else {
            panic!("not links: {definition:?}");
        };
        assert_eq!(
            uri::to_path(&links[0].target_uri).unwrap(),
            self.root.join("src/b.rs")
        );
        let name = B.find("make_counter").unwrap();
        let range = LineIndex::new(B).range(B, name..name + "make_counter".len(), self.encoding);
        assert_eq!(links[0].target_selection_range, range);
        took
    }

    /// Over the method call: its signature and its documentation.
    fn hover(&self, document: &Document) -> Duration {
        let method = A.find("doubled()").unwrap();
        let params = HoverParams {
            text_document_position_params: at(document, self.position(A, method)),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let (hover, took) = self.ask::<HoverRequest>(&params);
        let hover = hover.unwrap();
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("not markup: {hover:?}");
        };
        assert!(
            markup.value.contains("pub fn doubled(&self) -> i32"),
            "{}",
            markup.value
        );
        assert!(
            markup.value.contains("Twice the value."),
            "{}",
            markup.value
        );
        took
    }

    /// The type error, on the string literal, once a save has the crate checked.
    fn diagnostics(
        &self,
        document: &Document,
        diagnostics: &mut Notifications<PublishDiagnosticsParams>,
    ) -> Duration {
        let literal = A.find("\"not a number\"").unwrap();
        let length = "\"not a number\"".len();
        let expected = LineIndex::new(A).range(A, literal..literal + length, self.encoding);
        let started = Instant::now();
        self.client
            .notify::<DidSaveTextDocument>(&document.save(None))
            .unwrap();
        let diagnostic = wait(300, async {
            while let Some(published) = diagnostics.next().await {
                if published.uri != *document.uri() {
                    continue;
                }
                let found = published
                    .diagnostics
                    .iter()
                    .find(|diagnostic| diagnostic.range == expected)
                    .cloned();
                if let Some(diagnostic) = found {
                    return diagnostic;
                }
            }
            panic!("the server stopped publishing diagnostics");
        });
        let took = started.elapsed();
        assert_eq!(
            diagnostic.severity,
            Some(DiagnosticSeverity::ERROR),
            "{diagnostic:?}"
        );
        took
    }

    /// After `counter.`, typed into the open document.
    fn completion(&self, document: &mut Document) -> Duration {
        let mut text = IndexedText::new(A.to_owned());
        let edit = text.edit(A.len()..A.len(), MORE, self.encoding).unwrap();
        self.client
            .notify::<DidChangeTextDocument>(&document.change([edit]))
            .unwrap();
        let dot = text.as_str().rfind("counter.").unwrap() + "counter.".len();
        let params = CompletionParams {
            text_document_position: at(document, text.position(dot, self.encoding)),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".to_owned()),
            }),
        };
        let (completion, took) = self.ask::<Completion>(&params);
        let items = match completion.unwrap() {
            CompletionResponse::Array(items) => items,
            CompletionResponse::List(list) => list.items,
        };
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert!(
            labels.iter().any(|label| label.starts_with("doubled")),
            "{labels:?}"
        );
        took
    }

    /// Sends `R`, asked again if a reload overtakes it: the answer, and how long it took.
    fn ask<R: Request>(&self, params: &R::Params) -> (R::Result, Duration) {
        let started = Instant::now();
        let answer = wait(60, self.client.request_retrying::<R>(params)).unwrap();
        (answer, started.elapsed())
    }

    fn position(&self, text: &str, offset: usize) -> Position {
        LineIndex::new(text).position(text, offset, self.encoding)
    }
}

/// The two-module crate, made afresh under the checkout's `target`.
fn fixture() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lsp-fixtures")
        .join(format!("rust-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    for (path, text) in [
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", LIB),
        ("src/a.rs", A),
        ("src/b.rs", B),
    ] {
        fs::write(root.join(path), text).unwrap();
    }
    // Spelled as the server will spell it, without the `..`.
    fs::canonicalize(&root).unwrap()
}

fn at(document: &Document, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier::new(document.uri().clone()),
        position,
    }
}
