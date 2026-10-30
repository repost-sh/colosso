//! The Languages settings page: which languages get a language server, for go to definition,
//! hover, diagnostics, completions and the rest, and how the servers are run.

use colosso_lsp::servers::{PYREFLY, RUST_ANALYZER, TYPESCRIPT};
use gpui_kit::App;

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::settings::{self, Scope, SetMostLanguageServers};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![servers(), running(cx)]
}

/// The choices for how many language servers run at once, no limit first.
const MOST_SERVERS: [Option<usize>; 7] =
    [None, Some(1), Some(2), Some(3), Some(4), Some(6), Some(8)];

/// How a choice of how many language servers run at once reads.
fn most_label(most: Option<usize>) -> String {
    most.map_or_else(|| "No limit".to_owned(), |most| most.to_string())
}

/// Each language with a server: on or off everywhere. The status bar's menu does it for one
/// project.
fn servers() -> SettingsSection {
    SettingsSection::new("Language servers")
        .row(
            SettingRow::new("Rust")
                .description(
                    "Go to definition, hover, diagnostics, completions and symbols in Rust \
                     files, from rust-analyzer: the one on PATH, else one downloaded the first \
                     time a Rust file opens. Each server's menu in the status bar turns it off \
                     for one project only.",
                )
                .control(NativeSwitch::new(
                    "rust-analyzer",
                    |settings| settings.runs_server(RUST_ANALYZER.name, None),
                    |settings, on| settings.set_server(RUST_ANALYZER.name, &Scope::Everywhere, on),
                )),
        )
        .row(
            SettingRow::new("TypeScript and JavaScript")
                .description(
                    "The same in TypeScript and JavaScript files, from TypeScript 7's own \
                     server: the project's own, when it has TypeScript 7 or newer in \
                     node_modules, else one downloaded the first time. A project on an older \
                     TypeScript gets vtsls instead, run by the machine's Node when there is \
                     one, so what the editor says agrees with what the project builds.",
                )
                .control(NativeSwitch::new(
                    "typescript",
                    |settings| settings.runs_server(TYPESCRIPT.name, None),
                    |settings, on| settings.set_server(TYPESCRIPT.name, &Scope::Everywhere, on),
                )),
        )
        .row(
            SettingRow::new("Python")
                .description(
                    "The same in Python files, from Pyrefly: the project's own in its .venv, \
                     else the one on PATH, else one downloaded the first time.",
                )
                .control(NativeSwitch::new(
                    "pyrefly",
                    |settings| settings.runs_server(PYREFLY.name, None),
                    |settings, on| settings.set_server(PYREFLY.name, &Scope::Everywhere, on),
                )),
        )
}

/// How the servers are run: stopped when idle, and how many at once.
fn running(cx: &App) -> SettingsSection {
    let current = settings::get(cx);
    SettingsSection::new("Running")
        .row(
            SettingRow::new("Stop when idle")
                .description(
                    "Stop a language server ten minutes after the last file it knows is \
                     closed. It starts, and indexes, again with the next one.",
                )
                .control(NativeSwitch::new(
                    "stop-idle-language-servers",
                    |settings| settings.stop_idle_language_servers,
                    |settings, on| settings.stop_idle_language_servers = on,
                )),
        )
        .row(
            SettingRow::new("Most at once")
                .description(
                    "Each worktree with a Rust, TypeScript, JavaScript or Python file open runs \
                     its own server; rust-analyzer can take several GB of memory on a large \
                     project. \
                     With a limit, the one used least recently stops, and indexes again when \
                     it's next needed.",
                )
                .control(MenuButton::new(
                    "most-language-servers",
                    most_label(current.most_language_servers),
                    |cx| {
                        let most = settings::get(cx).most_language_servers;
                        settings::checked_menu(MOST_SERVERS, most, most_label, |choice| {
                            Box::new(SetMostLanguageServers(choice))
                        })
                    },
                )),
        )
}
