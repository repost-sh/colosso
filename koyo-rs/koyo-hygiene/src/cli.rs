use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "koyo-hygiene [--fix] [PATH...] | --prune | --stats

Holds Koyo's source to two rules. Rust (.rs) and TypeScript (.ts, .tsx) have no plain
comments. No import between the koyo crate's top-level modules closes a new cycle:
koyo-rs/module-edges.txt lists the edges on a cycle today, and only shrinks.
With no argument it checks both, the comments in the koyo-rs/koyo, koyo-rs/koyo-* and
koyo-js files git lists.
A PATH is one file, or a folder searched for those files, whose comments alone are checked.
--fix deletes every plain comment it can prove is only a comment; // SAFETY: comments stay.
--prune takes the edges no longer on a cycle off koyo-rs/module-edges.txt.
--stats prints the import graph's size, its largest cycle, and the edges on a cycle with the
fewest import sites: the cheapest cuts.
Exit 0 when nothing is left to report, 1 when findings remain, 2 when the tool itself failed.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Check,
    Fix,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// No argument: the comment check over the files git lists, and the import check.
    Everything,
    /// The comment check or fix over the PATHs, or over the files git lists when none is given.
    Comments {
        mode: Mode,
        paths: Vec<PathBuf>,
    },
    Prune,
    Stats,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Run(Command),
    Help,
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Request, String> {
    let mut mode = Mode::Check;
    let mut alone = Vec::new();
    let mut paths = Vec::new();
    let mut only_paths_follow = false;
    for argument in arguments {
        match argument.to_str() {
            Some(_) if only_paths_follow => paths.push(PathBuf::from(argument)),
            Some("--") => only_paths_follow = true,
            Some("--fix") => mode = Mode::Fix,
            Some("--prune") => alone.push(Command::Prune),
            Some("--stats") => alone.push(Command::Stats),
            Some("--help" | "-h") => return Ok(Request::Help),
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option}; see --help"));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    let comments_only = mode == Mode::Fix || !paths.is_empty();
    let command = match alone.as_slice() {
        [] if comments_only => Command::Comments { mode, paths },
        [] => Command::Everything,
        [Command::Prune] if !comments_only => Command::Prune,
        [Command::Stats] if !comments_only => Command::Stats,
        _ => return Err("--prune and --stats each go alone; see --help".to_owned()),
    };
    Ok(Request::Run(command))
}
