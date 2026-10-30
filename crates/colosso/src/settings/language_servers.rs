//! Which language servers run: each one on or off everywhere, and off in some projects
//! though on elsewhere. In `settings.json` as
//!
//! ```json
//! "language_servers": { "rust-analyzer": { "enabled": true, "off_in": ["/path/to/project"] } }
//! ```
//!
//! A server the file doesn't name runs, as each does unless it's turned off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use colosso_lsp::servers::{PYREFLY, RUST_ANALYZER, TYPESCRIPT};
use gpui_kit::Action;
use serde::{Deserialize, Serialize};

use super::Settings;

/// What the settings say of one language server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSettings {
    /// Whether it runs at all.
    pub enabled: bool,
    /// The projects, by their main checkout, it doesn't run in.
    pub off_in: Vec<PathBuf>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        ServerSettings {
            enabled: true,
            off_in: Vec::new(),
        }
    }
}

/// The servers the settings name, by name.
pub type LanguageServers = BTreeMap<String, ServerSettings>;

/// Every server the app runs, each on: so the file lists them for whoever edits it.
pub fn every_server() -> LanguageServers {
    ([RUST_ANALYZER.name, TYPESCRIPT.name, PYREFLY.name].into_iter())
        .map(|name| (name.to_owned(), ServerSettings::default()))
        .collect()
}

impl Settings {
    /// Lists every server the app runs, as a file from before one was added doesn't.
    pub(super) fn list_every_server(&mut self) {
        for (name, server) in every_server() {
            self.language_servers.entry(name).or_insert(server);
        }
    }
}

/// Where a language server is turned on or off.
#[derive(Clone, Debug, PartialEq)]
pub enum Scope {
    Everywhere,
    /// In the project whose main checkout this is.
    Project(PathBuf),
}

/// Turns the language server `name` on or off, in `scope`.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetLanguageServer {
    pub name: &'static str,
    pub scope: Scope,
    pub on: bool,
}

impl Settings {
    /// Whether the language server `name` runs in `project`, by its main checkout; anywhere,
    /// without one.
    pub fn runs_server(&self, name: &str, project: Option<&Path>) -> bool {
        self.language_servers.get(name).is_none_or(|server| {
            server.enabled && project.is_none_or(|project| !server.off_in.iter().any(|off| off == project))
        })
    }

    /// Turns the language server `name` on or off in `scope`. Turned on in a project while
    /// it's off everywhere, it's on everywhere but in the projects it was turned off in.
    pub fn set_server(&mut self, name: &str, scope: &Scope, on: bool) {
        let server = self.language_servers.entry(name.to_owned()).or_default();
        match scope {
            Scope::Everywhere => server.enabled = on,
            Scope::Project(project) => {
                server.off_in.retain(|off| off != project);
                if on {
                    server.enabled = true;
                } else {
                    server.off_in.push(project.clone());
                }
            }
        }
    }
}
