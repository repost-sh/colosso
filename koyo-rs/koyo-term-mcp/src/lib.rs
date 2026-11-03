//! `koyo mcp`: Koyo's MCP server (`docs/app-control.md`), for an agent to start over
//! stdio. It lists the app's tools and the terminals' (`docs/terminals.md` 8) as one server.
//! App calls go to the running app over its control socket, through the session's
//! [`Backend`]; terminal calls go to the daemon as a relay client, running where the agent
//! runs so `current` means the agent's worktree.
//!
//! The handler, [`Server`], is the same whatever carries it: [`main`] serves it on stdin and
//! stdout with [`socket_backend`] and a [`Launcher`] for `app_launch`; the app can serve it
//! over Streamable HTTP with a backend onto its own control pipeline, and no launcher.

mod app;
mod backend;
mod guide;
pub mod http;
mod launch;
mod resources;
mod server;
mod socket;
mod subscriptions;
mod tools;
mod watch;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use koyo_app_control::Endpoint;
use koyo_term_control::catalog::Scope;
use koyo_term_control::{DaemonCommand, TermDir};

pub use backend::{Backend, Connect, NoticeSink};
pub use launch::{LAUNCH_WAIT, Launcher, Spawner};
pub use server::{Server, serve};
pub use socket::socket_backend;

/// What the server needs to reach the daemon, and who its client is.
#[derive(Clone, Debug)]
pub struct Config {
    pub dir: TermDir,
    /// How to start the daemon when none runs.
    pub daemon: DaemonCommand,
    /// What the client may do in the terminals; the app decides its own scopes.
    pub grants: Vec<Scope>,
    /// Where the agent runs: the worktree `current` means.
    pub cwd: Option<PathBuf>,
    /// The agent's own session, when the app launched it.
    pub session: Option<String>,
    /// The name to call as, instead of the MCP client's own (`--client`).
    pub client: Option<String>,
}

const USAGE: &str = "usage: koyo mcp [--scopes terminal.read,terminal.write,...] [--client NAME]

Serves Koyo as an MCP server over stdio: the running app's projects, worktrees, tabs,
panes, files, browser pages and settings, through its control socket, and the terminals of
the worktree it runs in. app_launch starts the app when it isn't running.
--scopes  what the client may do in the terminals (default terminal.read,terminal.write,
          terminal.manage; terminal.destructive also closes terminals whose program still
          runs). The app decides what the client may do in it.
--client  the name to call the app and the daemon as, instead of the one the MCP client
          gives in initialize";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
struct Options {
    grants: Vec<Scope>,
    client: Option<String>,
}

/// Reads the arguments after `mcp`; `None` when they ask for the usage.
fn options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options {
        grants: Scope::AGENT.to_vec(),
        client: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scopes" => match args.next().as_deref().map(scopes) {
                Some(given) => options.grants = given?,
                None => return Err("--scopes needs a list".to_owned()),
            },
            "--client" => match args.next().filter(|name| !name.trim().is_empty()) {
                Some(name) => options.client = Some(name),
                None => return Err("--client needs a name".to_owned()),
            },
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(Some(options))
}

/// Runs the server on stdin and stdout until the client goes; `daemon` starts the daemon
/// when none runs, and `app_launch` starts this executable as the app.
pub fn main(args: impl Iterator<Item = String>, daemon: DaemonCommand) -> ExitCode {
    let options = match options(args) {
        Ok(Some(options)) => options,
        Ok(None) => {
            let _ = writeln!(std::io::stderr(), "{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => return fail(&error),
    };
    let Some(dir) = TermDir::from_env() else {
        return fail("there's no data folder to find the terminal daemon in");
    };
    let config = Config {
        dir,
        daemon,
        grants: options.grants,
        cwd: std::env::current_dir().ok(),
        session: std::env::var("KOYO_AGENT_SESSION").ok(),
        client: options.client,
    };
    let endpoint = Endpoint::from_env();
    let launcher = Launcher::this_app(endpoint.clone());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("couldn't start: {error}")),
    };
    let server = Server::new(config, &socket_backend(endpoint), Some(launcher));
    match runtime.block_on(serve(server, rmcp::transport::stdio())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

/// Scopes by name, `terminal.read` or `read`, separated by commas.
fn scopes(list: &str) -> Result<Vec<Scope>, String> {
    let all = [Scope::Read, Scope::Write, Scope::Manage, Scope::Destructive];
    (list
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty()))
    .map(|name| {
        (all.iter().copied())
            .find(|scope| {
                scope.name() == name || scope.name().strip_prefix("terminal.") == Some(name)
            })
            .ok_or_else(|| format!("there's no scope called {name}"))
    })
    .collect()
}

fn fail(message: &str) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "koyo mcp: {message}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_client_flag_names_the_caller() {
        let read = |args: &[&str]| options(args.iter().map(|arg| (*arg).to_owned()));
        let named = read(&["--client", "night-shift", "--scopes", "read"]).unwrap();
        assert_eq!(
            named,
            Some(Options {
                grants: vec![Scope::Read],
                client: Some("night-shift".to_owned()),
            })
        );
        assert_eq!(read(&[]).unwrap().unwrap().client, None);
        assert!(read(&["--client"]).is_err());
        assert!(read(&["--client", " "]).is_err());
        assert_eq!(read(&["--help"]), Ok(None));
    }

    #[test]
    fn scopes_are_read_by_either_name() {
        assert_eq!(
            scopes("read, terminal.write,destructive").unwrap(),
            [Scope::Read, Scope::Write, Scope::Destructive]
        );
        assert!(scopes("everything").is_err());
    }
}
