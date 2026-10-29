//! Language servers behind the Files panel's viewer: a rust-analyzer, a vtsls or a
//! basedpyright per checkout, started when the first Rust, TypeScript or Python file of the
//! checkout is shown, kept in step with the files shown, and asked where a symbol is defined,
//! what it is, and what's wrong with the code.
//!
//! - [`Registry`]: the servers by kind and checkout, how many may run at once and which one
//!   stops for another, and stopping one that has had no file open for a while.
//! - [`Server`]: one server's life, started, followed as it indexes, restarted when it
//!   crashes, stopped; the files open on it (`sync`), what it published for them
//!   (`diagnostics`), and the files it asked to hear about (`watch`).
//! - `providers`: the editor's hooks, go to definition and hover, positions converted with
//!   the encoding the server chose; `completions`, the words that could complete the one
//!   being typed, ranked here as it's typed on; `code_actions`, what could be done about the
//!   code, and `edits`, the chosen one's edit applied; `navigate`, a definition in another
//!   file turned into a place in it for the viewer.
//! - `status`: the selected checkout's servers in the status bar, each with its menu;
//!   `server_log`, each server's own log, which the menu shows.

mod code_actions;
mod completions;
mod diagnostics;
mod edits;
mod navigate;
mod providers;
mod registry;
mod server;
mod server_log;
mod status;
mod sync;
#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;
mod watch;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use colosso_lsp::Launch;
use colosso_lsp::servers::{BASEDPYRIGHT, Curl, NodeServer, NpmCli, RUST_ANALYZER, VTSLS, node};
use gpui_kit::component::input::{GoToDefinition, ShowCompletions};
use gpui_kit::{App, KeyBinding};

pub use providers::Attached;
pub use registry::{Languages, attach, forget, install, locate};
pub use server::State;
pub use status::ServersStatus;

use crate::settings;

/// A server stops once it has had no file open for this long, if the settings ask.
const IDLE: Duration = Duration::from_mins(10);
/// A server that crashes is restarted up to this many times within [`CRASH_WINDOW`], and
/// left crashed after that until the user retries.
const MOST_RESTARTS: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_mins(5);
/// How long a server is given to shut down, at each step, before it's killed.
const SHUTDOWN: Duration = Duration::from_secs(2);

/// How long to wait before restarting a server that has crashed `crashes` times lately:
/// one second, then two, then four.
fn backoff(crashes: usize) -> Duration {
    Duration::from_secs(1 << crashes.saturating_sub(1).min(4))
}

/// The kinds of server the app runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Rust,
    /// vtsls, for TypeScript and JavaScript both.
    TypeScript,
    /// basedpyright.
    Python,
}

impl Kind {
    /// The server for the file at `path`, by its extension, if any knows it.
    pub fn of(path: &Path) -> Option<Kind> {
        file(path).map(|(kind, _)| kind)
    }

    /// The server's name, as the status bar, the settings and the log say it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rust => RUST_ANALYZER.name,
            Kind::TypeScript => VTSLS.name,
            Kind::Python => BASEDPYRIGHT.name,
        }
    }

    /// The language it's for, as the status bar says it.
    pub fn language(self) -> &'static str {
        match self {
            Kind::Rust => "Rust",
            Kind::TypeScript => "TypeScript",
            Kind::Python => "Python",
        }
    }

    /// Whether the server says when it's done loading, with `experimental/serverStatus` as
    /// rust-analyzer does; one that doesn't is taken as ready once it's initialized and
    /// reports no work.
    fn reports_status(self) -> bool {
        matches!(self, Kind::Rust)
    }
}

/// The server for the file at `path`, and the language the file is opened on it as, by
/// the file's extension: the one table of both.
fn file(path: &Path) -> Option<(Kind, &'static str)> {
    const KNOWN: [(&str, Kind, &str); 11] = [
        ("rs", Kind::Rust, "rust"),
        ("ts", Kind::TypeScript, "typescript"),
        ("mts", Kind::TypeScript, "typescript"),
        ("cts", Kind::TypeScript, "typescript"),
        ("tsx", Kind::TypeScript, "typescriptreact"),
        ("js", Kind::TypeScript, "javascript"),
        ("mjs", Kind::TypeScript, "javascript"),
        ("cjs", Kind::TypeScript, "javascript"),
        ("jsx", Kind::TypeScript, "javascriptreact"),
        ("py", Kind::Python, "python"),
        ("pyi", Kind::Python, "python"),
    ];
    let extension = path.extension()?.to_str()?;
    (KNOWN.iter().find(|(known, ..)| known.eq_ignore_ascii_case(extension)))
        .map(|&(_, kind, language_id)| (kind, language_id))
}

/// The language the file at `path` is opened on its server as.
fn language_id(path: &Path) -> &'static str {
    file(path).map_or("plaintext", |(_, language_id)| language_id)
}

/// Why a server couldn't be started: a few words for the status bar, when there are any
/// short enough to say beside its name, and the whole story.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub words: Option<&'static str>,
    pub why: String,
}

impl From<String> for Failure {
    fn from(why: String) -> Failure {
        Failure { words: None, why }
    }
}

/// Finds a kind's server and says how to start it for the checkout at a root. It may
/// download the server, so it's called on a background thread.
pub type Launcher = Arc<dyn Fn(Kind, &Path) -> Result<Launch, Failure> + Send + Sync>;

/// Runs the language servers the app knows, fetched into its data folder when they aren't
/// on PATH; binds F12 to go to a definition and ctrl-space to complete; and has the status
/// bar's menus act.
pub fn init(cx: &mut App) {
    install(managed(true), cx);
    bind_keys(cx);
    status::register(cx);
}

/// Runs rust-analyzer from PATH when there's a working one there and `path_first`, and
/// else from the app's data folder, fetched into it the first time; and the Node servers
/// from the data folder, installed into it by npm the first time, with the machine's Node.
fn managed(path_first: bool) -> Launcher {
    let data = settings::data_dir();
    Arc::new(move |kind, root| {
        let data = (data.as_deref()).ok_or_else(|| "there's no folder to fetch it into".to_owned())?;
        match kind {
            Kind::Rust => rust_analyzer(data, path_first, root),
            Kind::TypeScript => node_server(&VTSLS, data, root),
            Kind::Python => node_server(&BASEDPYRIGHT, data, root),
        }
    })
}

fn rust_analyzer(data: &Path, path_first: bool, root: &Path) -> Result<Launch, Failure> {
    let binary = if path_first {
        RUST_ANALYZER.binary(data, &Curl)
    } else {
        RUST_ANALYZER.install(data, &Curl)
    };
    let binary = binary.map_err(|error| error.to_string())?;
    Ok(RUST_ANALYZER.launch(binary, root))
}

/// `server` run by the machine's Node, installed into `data` by npm the first time.
fn node_server(server: &NodeServer, data: &Path, root: &Path) -> Result<Launch, Failure> {
    let node = node().ok_or(Failure {
        words: Some("needs Node"),
        why: "Node isn't on PATH, nor where Homebrew or the nodejs.org installer put it; \
              install it from nodejs.org or with `brew install node`"
            .to_owned(),
    })?;
    let npm = NpmCli { node: node.clone() };
    let script = server.script(data, &npm).map_err(|error| error.to_string())?;
    Ok(server.launch(node, &script, root))
}

/// F12 goes to the definition of what's under the cursor; ctrl-space asks for completions
/// without typing.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f12", GoToDefinition, Some("Input")),
        KeyBinding::new("ctrl-space", ShowCompletions, Some("Input")),
    ]);
}
