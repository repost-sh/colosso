use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::blocking::{self, Calls};
use crate::cli::{Command, Mode, Request, USAGE, parse};
use crate::failure::Failure;
use crate::files::{Source, checkout_root, listed_by_git, named};
use crate::imports::{self, Graph, LIST};
use crate::pool::map_in_order;
use crate::process::{FileReport, process};
use crate::report::{counted, write_findings, write_fix_summary};
use crate::shipped;

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
    let (reports, failures) = match command {
        Command::Everything => {
            let root = checkout_root()?;
            let (mut reports, mut failures) = process_all(&listed_by_git(&root)?, Mode::Check)?;
            let checks = match shipped_build(&root) {
                Ok((graph, calls)) => vec![
                    imports::check(&root, &graph),
                    blocking::check(&root, &calls),
                ],
                Err(failure) => vec![Err(failure)],
            };
            for check in checks {
                match check {
                    Ok(found) => reports.extend(found),
                    Err(failure) => failures.push(failure),
                }
            }
            reports.sort_by(|left, right| left.path.cmp(&right.path));
            (reports, failures)
        }
        Command::Comments { mode, paths } => comments(mode, &paths, out)?,
        Command::Prune => {
            let root = checkout_root()?;
            let (graph, calls) = shipped_build(&root)?;
            let (pruned, mut reports) = imports::prune(&root, &graph)?;
            writeln!(out, "Pruned {} from {LIST}.", counted(pruned, "edge"))?;
            let (lowered, debt) = blocking::prune(&root, &calls)?;
            for (list, lines) in lowered {
                writeln!(out, "Lowered {} of {list}.", counted(lines, "line"))?;
            }
            reports.extend(debt);
            reports.sort_by(|left, right| left.path.cmp(&right.path));
            (reports, Vec::new())
        }
        Command::Stats => {
            let (graph, calls) = shipped_build(&checkout_root()?)?;
            imports::stats(&graph, out)?;
            calls.write_stats(out)?;
            return Ok(Exit::Clean);
        }
    };
    write_findings(out, &reports)?;
    for failure in &failures {
        writeln!(err, "koyo-hygiene: {failure}")?;
    }
    Ok(exit_for(&reports, failures.len()))
}

/// The shipped build at `root`, walked once for the two checks that read it: the koyo crate's
/// import graph, and its blocking calls and the browser host's, which runs on the app's main
/// thread too.
fn shipped_build(root: &Path) -> Result<(Graph, Calls), Failure> {
    let reads = shipped::walk(root, shipped::KOYO, |file, text, code| {
        (
            imports::read(file, text, code),
            blocking::read(file, text, code),
        )
    })?;
    let (imports, mut calls): (Vec<_>, Vec<_>) = reads.into_iter().unzip();
    calls.extend(shipped::walk(root, shipped::BROWSER_HOST, blocking::read)?);
    Ok((imports::graph(imports), Calls::of(calls)))
}

fn comments(
    mode: Mode,
    paths: &[PathBuf],
    out: &mut impl Write,
) -> Result<(Vec<FileReport>, Vec<Failure>), Failure> {
    let sources = if paths.is_empty() {
        listed_by_git(&checkout_root()?)?
    } else {
        named(paths)?
    };
    let (reports, failures) = process_all(&sources, mode)?;
    if mode == Mode::Fix {
        let changed = reports.iter().filter(|report| report.deleted > 0).count();
        write_fix_summary(
            out,
            reports.iter().map(|report| report.deleted).sum(),
            changed,
        )?;
    }
    Ok((reports, failures))
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
