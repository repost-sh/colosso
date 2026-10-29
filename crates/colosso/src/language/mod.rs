//! Language servers behind the Files panel's viewer: a rust-analyzer per checkout, started
//! when the first Rust file of the checkout is shown, kept in step with the files shown, and
//! asked where a symbol is defined, what it is, and what's wrong with the code.
//!
//! - [`Registry`]: the servers by kind and checkout, how many may run at once and which one
//!   stops for another, and stopping one that has had no file open for a while.
//! - [`Server`]: one server's life, started, followed as it indexes, restarted when it
//!   crashes, stopped; the files open on it (`sync`), what it published for them
//!   (`diagnostics`), and the files it asked to hear about (`watch`).
//! - `providers`: the editor's hooks, go to definition and hover, positions converted with
//!   the encoding the server chose; `navigate`, a definition in another file turned into a
//!   place in it for the viewer.
//! - `status`: the selected checkout's servers in the status bar, each with its menu;
//!   `server_log`, each server's own log, which the menu shows.

mod diagnostics;
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
use colosso_lsp::servers::{Curl, RUST_ANALYZER};
use gpui_kit::component::input::GoToDefinition;
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
}

impl Kind {
    /// The server for the file at `path`, by its extension, if any knows it.
    pub fn of(path: &Path) -> Option<Kind> {
        let extension = path.extension()?.to_str()?;
        extension.eq_ignore_ascii_case("rs").then_some(Kind::Rust)
    }

    /// The server's name, as the status bar, the settings and the log say it.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Rust => "rust-analyzer",
        }
    }

    /// The language it's for, as the status bar says it.
    pub fn language(self) -> &'static str {
        match self {
            Kind::Rust => "Rust",
        }
    }

    /// The language a file is opened on the server as.
    fn language_id(self) -> &'static str {
        match self {
            Kind::Rust => "rust",
        }
    }
}

/// Finds a kind's server and says how to start it for the checkout at a root. It may
/// download the server, so it's called on a background thread.
pub type Launcher = Arc<dyn Fn(Kind, &Path) -> Result<Launch, String> + Send + Sync>;

/// Runs the language servers the app knows, fetched into its data folder when they aren't
/// on PATH; binds F12 to go to a definition; and has the status bar's menus act.
pub fn init(cx: &mut App) {
    install(managed(true), cx);
    bind_keys(cx);
    status::register(cx);
}

/// Runs each server from PATH when there's a working one there and `path_first`, and else
/// from the app's data folder, fetched into it the first time.
fn managed(path_first: bool) -> Launcher {
    let data = settings::data_dir();
    Arc::new(move |kind, root| match kind {
        Kind::Rust => {
            let data = data
                .as_deref()
                .ok_or("there's no folder to fetch it into")?;
            let binary = if path_first {
                RUST_ANALYZER.binary(data, &Curl)
            } else {
                RUST_ANALYZER.install(data, &Curl)
            };
            let binary = binary.map_err(|error| error.to_string())?;
            Ok(RUST_ANALYZER.launch(binary, root))
        }
    })
}

/// F12 goes to the definition of what's under the cursor.
fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f12", GoToDefinition, Some("Input"))]);
}
