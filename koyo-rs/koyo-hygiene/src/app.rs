use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use crate::cli::{Mode, Request, USAGE, parse};
use crate::failure::Failure;
use crate::files::{Source, listed_by_git, named};
use crate::pool::map_in_order;
use crate::process::{FileReport, process};
use crate::report::{write_findings, write_fix_summary};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    Clean,
    FindingsRemain,
    ToolFailed,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        match exit {
            Exit::Clean => Self::SUCCESS,
            Exit::FindingsRemain => Self::from(1),
            Exit::ToolFailed => Self::from(2),
        }
    }
}

pub fn run(
    arguments: impl IntoIterator<Item = OsString>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> ExitCode {
    match execute(arguments, out, err) {
        Ok(exit) => exit.into(),
        Err(failure) => {
            let _ = writeln!(err, "koyo-hygiene: {failure}");
            Exit::ToolFailed.into()
        }
    }
}

fn execute(
    arguments: impl IntoIterator<Item = OsString>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<Exit, Failure> {
    let command = match parse(arguments) {
        Ok(Request::Run(command)) => command,
        Ok(Request::Help) => {
            writeln!(out, "{USAGE}")?;
            return Ok(Exit::Clean);
        }
        Err(message) => return Err(Failure::new("usage", message)),
    };
    let sources = if command.paths.is_empty() {
        listed_by_git()?
    } else {
        named(&command.paths)?
    };
    let (reports, failures) = process_all(&sources, command.mode)?;
    if command.mode == Mode::Fix {
        let changed = reports.iter().filter(|report| report.deleted > 0).count();
        write_fix_summary(
            out,
            reports.iter().map(|report| report.deleted).sum(),
            changed,
        )?;
    }
    write_findings(out, &reports)?;
    for failure in &failures {
        writeln!(err, "koyo-hygiene: {failure}")?;
    }
    Ok(exit_for(&reports, failures.len()))
}

fn process_all(sources: &[Source], mode: Mode) -> Result<(Vec<FileReport>, Vec<Failure>), Failure> {
    let outcomes = map_in_order(sources, |source| process(source, mode))
        .map_err(|failure| Failure::new("threads", failure))?;
    let mut reports = Vec::new();
    let mut failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(Some(report)) => reports.push(report),
            Ok(None) => {}
            Err(failure) => failures.push(failure),
        }
    }
    Ok((reports, failures))
}

fn exit_for(reports: &[FileReport], failures: usize) -> Exit {
    if failures > 0 {
        Exit::ToolFailed
    } else if reports.iter().any(|report| !report.findings.is_empty()) {
        Exit::FindingsRemain
    } else {
        Exit::Clean
    }
}
