use std::ffi::OsString;
use std::path::PathBuf;

pub const USAGE: &str = "koyo-hygiene [--fix] [PATH...]

Finds the plain comments Koyo's source must not have, in Rust (.rs) and TypeScript (.ts, .tsx).
With no PATH it checks the koyo-rs/koyo, koyo-rs/koyo-* and koyo-js files git lists.
A PATH is one file, or a folder searched for those files.
--fix deletes every plain comment it can prove is only a comment; // SAFETY: comments stay.
Exit 0 when nothing is left to report, 1 when findings remain, 2 when the tool itself failed.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Check,
    Fix,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub mode: Mode,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Run(Command),
    Help,
}

pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Request, String> {
    let mut mode = Mode::Check;
    let mut paths = Vec::new();
    let mut only_paths_follow = false;
    for argument in arguments {
        match argument.to_str() {
            Some(_) if only_paths_follow => paths.push(PathBuf::from(argument)),
            Some("--") => only_paths_follow = true,
            Some("--fix") => mode = Mode::Fix,
            Some("--help" | "-h") => return Ok(Request::Help),
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option}; see --help"));
            }
            _ => paths.push(PathBuf::from(argument)),
        }
    }
    Ok(Request::Run(Command { mode, paths }))
}
