//! The client against the real vtsls and basedpyright: each installed by npm from the Node
//! on PATH into the data folder, started, given a file, and asked what a name is.
//!
//! It installs the packages the first time, so it runs only when asked:
//! `COLOSSO_LSP_IT=1 COLOSSO_DATA_DIR=$PWD/.superpowers/data cargo test -p colosso-lsp -- --ignored node_servers`

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use colosso_lsp::lsp_types::notification::DidOpenTextDocument;
use colosso_lsp::lsp_types::request::HoverRequest;
use colosso_lsp::lsp_types::{
    HoverContents, HoverParams, MarkedString, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};
use colosso_lsp::servers::{BASEDPYRIGHT, NpmCli, VTSLS, node};
use colosso_lsp::{Client, Document, LineIndex, LspError, uri, within};

use crate::common::{fixture, wait};

const TS: &str = "/** Greets `name`. */\nexport function greet(name: string): string {\n  \
    return `hi ${name}`;\n}\n\nexport const said = greet(\"you\");\n";
const PY: &str = "def greet(name: str) -> str:\n    \"\"\"Greets `name`.\"\"\"\n    \
    return f\"hi {name}\"\n\n\nsaid = greet(\"you\")\n";

#[test]
#[ignore = "installs and runs vtsls and basedpyright; see this file's header"]
fn vtsls_and_basedpyright_say_what_a_call_is() {
    if std::env::var("COLOSSO_LSP_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set COLOSSO_LSP_IT=1 to run against the Node servers");
        return;
    }
    let data = std::env::var_os("COLOSSO_DATA_DIR")
        .map(PathBuf::from)
        .expect("COLOSSO_DATA_DIR names a folder in the checkout to install into");
    let node = node().expect("Node on PATH");
    let npm = NpmCli { node: node.clone() };
    let runs = [
        (&VTSLS, "app.ts", "typescript", TS, "(name: string): string"),
        (&BASEDPYRIGHT, "tool.py", "python", PY, "(name: str) -> str"),
    ];
    for (server, file, language_id, text, signature) in runs {
        let took = Instant::now();
        let script = server.script(&data, &npm).unwrap();
        eprintln!("{}: installed, {:.1} s", server.name, took.elapsed().as_secs_f64());
        let root = fixture(server.name, &[(file, text)]);
        let took = Instant::now();
        let client = Client::start(&server.launch(node.clone(), &script, &root)).unwrap();
        wait(60, client.initialize(&root)).unwrap();
        eprintln!(
            "{}: initialized in {:.0} ms, speaking {:?}",
            server.name,
            took.elapsed().as_secs_f64() * 1000.,
            client.encoding()
        );
        let document = Document::new(uri::from_path(&root.join(file)).unwrap());
        client
            .notify::<DidOpenTextDocument>(&document.open(language_id, text))
            .unwrap();
        // The call at the end: the hover names the function's signature.
        let call = text.rfind("greet(").unwrap() + 1;
        let position = LineIndex::new(text).position(text, call, client.encoding());
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams::new(
                TextDocumentIdentifier::new(document.uri().clone()),
                position,
            ),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let took = Instant::now();
        let hover = wait(120, async {
            // Still loading its own library files for a moment after it initializes.
            loop {
                match client.request::<HoverRequest>(&params).await {
                    Ok(Some(hover)) => return hover,
                    Ok(None) | Err(LspError::Server { .. }) => {
                        // A moment's pause, on the client's own timer.
                        within(Duration::from_millis(250), std::future::pending::<()>()).await;
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        eprintln!(
            "{}: hover in {:.0} ms",
            server.name,
            took.elapsed().as_secs_f64() * 1000.
        );
        let said = match hover.contents {
            HoverContents::Markup(markup) => markup.value,
            HoverContents::Scalar(MarkedString::String(text)) => text,
            other => panic!("not text: {other:?}"),
        };
        assert!(said.contains(signature), "{said}");
        let exit = wait(60, client.shutdown(Duration::from_secs(10)));
        assert!(!exit.crashed && !exit.killed, "{exit:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
