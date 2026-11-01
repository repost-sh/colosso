//! `colosso mcp`: the terminals' MCP server, for an agent to start over stdio
//! (`docs/terminals.md` 8). It connects to the daemon as a relay client, running where the
//! agent runs so `current` means the agent's worktree, and carries out the catalog's
//! operations over that connection.

mod guide;
mod resources;
mod server;
mod watch;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use colosso_term_control::catalog::Scope;
use colosso_term_control::{DaemonCommand, TermDir};

pub use server::{Terminals, serve};

/// What the server needs to reach the daemon, and what its client may do.
#[derive(Clone, Debug)]
pub struct Config {
    pub dir: TermDir,
    /// How to start the daemon when none runs.
    pub daemon: DaemonCommand,
    pub grants: Vec<Scope>,
    /// Where the agent runs: the worktree `current` means.
    pub cwd: Option<PathBuf>,
    /// The agent's own session, when the app launched it.
    pub session: Option<String>,
}

const USAGE: &str = "usage: colosso mcp [--scopes terminal.read,terminal.write,...]

Serves the terminals of the worktree it runs in as an MCP server over stdio.
--scopes  what the client may do (default terminal.read,terminal.write,terminal.manage;
          terminal.destructive also closes terminals whose program still runs)";

/// Runs the server on stdin and stdout until the client goes; `daemon` starts the daemon
/// when none runs.
pub fn main(mut args: impl Iterator<Item = String>, daemon: DaemonCommand) -> ExitCode {
    let mut grants = Scope::AGENT.to_vec();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scopes" => match args.next().as_deref().map(scopes) {
                Some(Ok(given)) => grants = given,
                Some(Err(error)) => return fail(&error),
                None => return fail("--scopes needs a list"),
            },
            "-h" | "--help" => {
                let _ = writeln!(std::io::stderr(), "{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unknown argument {other}\n{USAGE}")),
        }
    }
    let Some(dir) = TermDir::from_env() else {
        return fail("there's no data folder to find the terminal daemon in");
    };
    let config = Config {
        dir,
        daemon,
        grants,
        cwd: std::env::current_dir().ok(),
        session: std::env::var("COLOSSO_AGENT_SESSION").ok(),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return fail(&format!("couldn't start: {error}")),
    };
    match runtime.block_on(serve(config, rmcp::transport::stdio())) {
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
    let _ = writeln!(std::io::stderr(), "colosso mcp: {message}");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_read_by_either_name() {
        assert_eq!(
            scopes("read, terminal.write,destructive").unwrap(),
            [Scope::Read, Scope::Write, Scope::Destructive]
        );
        assert!(scopes("everything").is_err());
    }
}
