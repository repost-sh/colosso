//! Language servers behind the Files panel's viewer: a rust-analyzer, a TypeScript server or
//! a Pyrefly per checkout, started when the first Rust, TypeScript or Python file of the
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
//!   code, and `edits`, the chosen one's edit applied; `inlay_hints`, the types and
//!   parameter names the server infers, shown in the text; `navigate`, a definition in
//!   another file turned into a place in it for the viewer; `locations`, a symbol's
//!   references, or its several definitions, each with its line, for the panel listing
//!   them; `rename`, a symbol renamed in place, here and in the other files; `symbols`,
//!   the file's or the checkout's, for the palettes that go to one; `semantic`, the
//!   server's own reading of the code colouring it over the grammar's.
//! - `status`: the selected checkout's servers in the status bar, each with its menu;
//!   `server_log`, each server's own log, which the menu shows.

mod code_actions;
mod completions;
mod diagnostics;
mod edits;
mod heard;
mod inlay_hints;
mod locations;
mod navigate;
mod providers;
mod registry;
mod rename;
mod semantic;
mod server;
mod server_log;
mod status;
mod symbols;
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
use colosso_lsp::servers::typescript::{self, Engine};
use colosso_lsp::servers::{
    Curl, NpmCli, PYREFLY, RUST_ANALYZER, ServerSpec, TYPESCRIPT, VTSLS, node, vtsls_settings,
};
use gpui_kit::component::input::{GoToDefinition, ShowCompletions};
use gpui_kit::{App, KeyBinding, actions};

pub use locations::Found;
pub use providers::Attached;
pub use rename::Prepared;
pub use symbols::{Symbol, serves, workspace_symbols};
pub use registry::{Languages, attach, forget, install, locate};
pub use server::State;
pub use status::ServersStatus;

use crate::settings;

actions!(language, [FindReferences, Rename]);

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
    /// TypeScript's own server, or vtsls for an older project, for TypeScript and JavaScript
    /// both.
    TypeScript,
    /// Pyrefly.
    Python,
}

impl Kind {
    /// The server for the file at `path`, by its extension, if any knows it.
    pub fn of(path: &Path) -> Option<Kind> {
        file(path).map(|(kind, _)| kind)
    }

    /// The name the status bar, the settings and the log know the kind's server by: the
    /// server's own, or "typescript" whichever of its two engines runs.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rust => RUST_ANALYZER.name,
            Kind::TypeScript => TYPESCRIPT.name,
            Kind::Python => PYREFLY.name,
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

/// Finds a kind's server and says how to start it for the checkout at a root, or says why
/// it couldn't. It may download the server, so it's called on a background thread.
pub type Launcher = Arc<dyn Fn(Kind, &Path) -> Result<Launch, String> + Send + Sync>;

/// Runs the language servers the app knows, each the project's own or PATH's when there's
/// a working one and else fetched into the app's data folder; binds F12 to go to a
/// definition, shift-F12 to list a symbol's references and ctrl-space to complete; and has
/// the status bar's menus act.
pub fn init(cx: &mut App) {
    install(managed(), cx);
    bind_keys(cx);
    status::register(cx);
}

/// Runs each kind's server from the project or PATH when there's a working one there, and
/// else from the app's data folder, fetched into it the first time; but a project still on
/// TypeScript 5 or 6 gets vtsls, installed by npm and run with the machine's Node.
fn managed() -> Launcher {
    let data = settings::data_dir();
    Arc::new(move |kind, root| {
        let data = (data.as_deref()).ok_or_else(|| "there's no folder to fetch it into".to_owned())?;
        match kind {
            Kind::Rust => binary_server(&RUST_ANALYZER, data, root),
            Kind::TypeScript => typescript_server(data, root),
            Kind::Python => binary_server(&PYREFLY, data, root),
        }
    })
}

/// `server`'s binary: the project's own or PATH's when one runs, else the managed one.
fn binary_server(server: &ServerSpec, data: &Path, root: &Path) -> Result<Launch, String> {
    let binary = server.binary(root, data, &Curl).map_err(|error| error.to_string())?;
    Ok(server.launch(binary, root))
}

/// TypeScript's own server, the project's or the managed one; but a checkout with a project
/// still on TypeScript 5 or 6 anywhere in it gets vtsls, installed by npm the first time and
/// running the newest such TypeScript, when there's a Node to run it: TypeScript 7's server
/// can't follow such a project's imports, and so what the editor says agrees with what the
/// project builds with.
fn typescript_server(data: &Path, root: &Path) -> Result<Launch, String> {
    let engine = typescript::engine(root);
    if let Engine::Vtsls { typescript } = &engine && let Some(node) = node() {
        let npm = NpmCli { node: node.clone() };
        let script = VTSLS.script(data, &npm).map_err(|error| error.to_string())?;
        return Ok(VTSLS.launch(node, &script, root, vtsls_settings(typescript)));
    }
    let tsc = match engine {
        Engine::Native { tsc: Some(tsc) } => tsc,
        _ => TYPESCRIPT.install(data, &Curl).map_err(|error| error.to_string())?,
    };
    Ok(TYPESCRIPT.launch(tsc, root))
}

/// F12 goes to the definition of what's under the cursor, shift-F12 lists its references,
/// F2 renames it; ctrl-space asks for completions without typing.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("f12", GoToDefinition, Some("Input")),
        KeyBinding::new("shift-f12", FindReferences, Some("Input")),
        KeyBinding::new("f2", Rename, Some("Input")),
        KeyBinding::new("ctrl-space", ShowCompletions, Some("Input")),
    ]);
}
