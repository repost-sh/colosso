//! The terminal daemon: owns every terminal's process, keeps them while no window is open,
//! and serves the app, the MCP relay and later a mobile client over its socket.
//! See `docs/terminals.md` section 5.
//!
//! Run as `colosso-termd [--dir <folder>] [--foreground]`, or as `colosso termd ...` from the
//! app's own executable, which is how the app starts it: the folder defaults to the app's
//! data folder (`COLOSSO_DATA_DIR`) plus `term`; `--foreground` logs to stderr, not the file.

mod claim;
mod connection;
mod listen;
mod logfile;
mod open;
mod outbound;
mod registry;
mod server;
mod session;
mod worktree;

use std::io::Write as _;
use std::process::ExitCode;

use colosso_term_control::TermDir;

/// Runs the daemon with `args` (what follows the program or subcommand name) until it's
/// stopped. Call it before anything else in the process sets a logger or starts a thread.
pub fn main(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut args = args.into_iter();
    let mut dir = None;
    let mut foreground = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => dir = args.next().map(TermDir::at),
            "--foreground" => foreground = true,
            other => {
                return fail(&format!(
                    "unknown argument {other}; usage: colosso-termd [--dir <folder>] [--foreground]"
                ));
            }
        }
    }
    let Some(dir) = dir.or_else(TermDir::from_env) else {
        return fail("no data folder: set COLOSSO_DATA_DIR or pass --dir");
    };
    // The engine writes its terminfo and shell integration files under the daemon's own
    // folder, not the user's cache, unless told otherwise.
    if std::env::var_os("GHOSTTY_VT_CACHE_DIR").is_none() {
        // SAFETY: called before the daemon starts any thread, as documented above.
        unsafe { std::env::set_var("GHOSTTY_VT_CACHE_DIR", dir.root().join("cache")) };
    }
    match listen::run(dir, foreground) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::error!("{error}");
            fail(&error.to_string())
        }
    }
}

fn fail(message: &str) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "colosso-termd: {message}");
    ExitCode::FAILURE
}
