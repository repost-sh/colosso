//! What the tests share: a fake server run from a script, the fake's own messages, waiting
//! on a future with a deadline, and the one probe of every real server.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use colosso_lsp::lsp_types::notification::{DidOpenTextDocument, Notification};
use colosso_lsp::lsp_types::request::{HoverRequest, Request};
use colosso_lsp::lsp_types::{
    HoverContents, HoverParams, MarkedString, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};
use colosso_lsp::{Client, Document, Launch, LineIndex, LspError, uri, within};
use futures::executor::block_on;
use serde_json::{Value, json};

/// A request the fake answers however its script says.
pub enum Echo {}

impl Request for Echo {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "fake/echo";
}

/// A request whose answer must be a number.
pub enum Number {}

impl Request for Number {
    type Params = Value;
    type Result = u32;
    const METHOD: &'static str = "fake/number";
}

/// What the fake received, sent back by its `echo` steps.
pub enum Received {}

impl Notification for Received {
    type Params = Value;
    const METHOD: &'static str = "fake/received";
}

/// The settings fake servers are started with.
pub fn settings() -> Value {
    json!({"fake": {"answer": 42}})
}

/// A fake server running `script`.
pub fn fake(script: &Value) -> Client {
    fake_logged(script, None)
}

/// A fake server running `script`, its stderr added to `log` if there's one.
pub fn fake_logged(script: &Value, log: Option<PathBuf>) -> Client {
    let launch = Launch {
        name: "fake".to_owned(),
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-lsp")),
        args: vec![script.to_string()],
        env: Vec::new(),
        cwd: root().to_owned(),
        settings: settings(),
        initialization_options: Some(json!({"from": "launch"})),
        log,
    };
    Client::start(&launch).unwrap()
}

/// This crate's folder, which the fake runs in and initializes as its workspace.
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// A workspace of `files` for a real server, under this workspace's `target`, spelled as
/// the server will spell it.
pub fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lsp-fixtures")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::canonicalize(&root).unwrap()
}

/// `future`'s output, failing the test if that takes longer than `seconds`.
pub fn wait<F: Future>(seconds: u64, future: F) -> F::Output {
    block_on(within(Duration::from_secs(seconds), future))
        .unwrap_or_else(|| panic!("timed out after {seconds}s"))
}

/// A function and a call of it, for a real server to say what the call is.
pub struct Call {
    pub file: &'static str,
    pub language_id: &'static str,
    pub text: &'static str,
    /// What the hover over the call must name.
    pub signature: &'static str,
}

pub const GREET_TS: Call = Call {
    file: "app.ts",
    language_id: "typescript",
    text: "/** Greets `name`. */\nexport function greet(name: string): string {\n  \
        return `hi ${name}`;\n}\n\nexport const said = greet(\"you\");\n",
    signature: "(name: string): string",
};

pub const GREET_PY: Call = Call {
    file: "tool.py",
    language_id: "python",
    text: "def greet(name: str) -> str:\n    \"\"\"Greets `name`.\"\"\"\n    \
        return f\"hi {name}\"\n\n\nsaid = greet(\"you\")\n",
    signature: "(name: str) -> str",
};

/// The data folder the tests against real servers install into, when they're asked for with
/// `COLOSSO_LSP_IT=1`; `what` names the server in the line that says they weren't.
pub fn data_dir(what: &str) -> Option<PathBuf> {
    if std::env::var("COLOSSO_LSP_IT").as_deref() != Ok("1") {
        eprintln!("skipped: set COLOSSO_LSP_IT=1 to run against {what}");
        return None;
    }
    let data = std::env::var_os("COLOSSO_DATA_DIR")
        .expect("COLOSSO_DATA_DIR names a folder in the checkout to install into");
    Some(PathBuf::from(data))
}

/// Starts a real server from `launch` for the workspace at `root`, opens `call`'s file on it
/// and asks what the call at the text's end is: the hover must name the function's
/// signature. Then shuts it down, and removes the workspace. Says how long each step took,
/// and what the server said on stderr, for a complaint about the settings it was given.
pub fn probe(launch: &Launch, root: &Path, call: &Call) {
    let name = &launch.name;
    let took = Instant::now();
    let client = Client::start(launch).unwrap();
    wait(60, client.initialize(root)).unwrap();
    eprintln!(
        "{name}: initialized in {:.0} ms, speaking {:?}",
        took.elapsed().as_secs_f64() * 1000.,
        client.encoding()
    );
    let document = Document::new(uri::from_path(&root.join(call.file)).unwrap());
    client
        .notify::<DidOpenTextDocument>(&document.open(call.language_id, call.text))
        .unwrap();
    let text = call.text;
    let at = text.rfind("greet(").unwrap() + 1;
    let position = LineIndex::new(text).position(text, at, client.encoding());
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
    eprintln!("{name}: hover in {:.0} ms", took.elapsed().as_secs_f64() * 1000.);
    let said = match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(MarkedString::String(text)) => text,
        other => panic!("not text: {other:?}"),
    };
    assert!(said.contains(call.signature), "{said}");
    let exit = wait(60, client.shutdown(Duration::from_secs(10)));
    assert!(!exit.crashed && !exit.killed, "{exit:?}");
    eprintln!("{name}: said {:?}", exit.stderr);
    let _ = std::fs::remove_dir_all(root);
}
