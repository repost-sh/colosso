//! What the tests share: a fake server run from a script, the fake's own messages, and
//! waiting on a future with a deadline.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colosso_lsp::lsp_types::notification::Notification;
use colosso_lsp::lsp_types::request::Request;
use colosso_lsp::{Client, Launch, within};
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
