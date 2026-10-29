//! Measures what the code editor costs the main thread on large files: opening them,
//! scrolling, typing, resizing and searching, each as the time from one input event through
//! the full frame drawn after it. The numbers are the baseline later editor work is judged
//! against; see README.md for the scenarios and their budgets.

mod fixtures;
mod generate;
mod harness;
mod repo;
mod report;
mod run;
mod scan;
mod scenarios;
mod stats;
mod suite;

use std::io::Write as _;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};

use crate::fixtures::{FIXTURES_DIR, RESULTS_DIR};
use crate::scenarios::Scenario;
use crate::stats::Deadline;
use crate::suite::Outcome;

const USAGE: &str = "\
usage: colosso-bench <command>

  all [--only TEXT,..] [--timeout SECS]
                                       make the fixtures, run every scenario (or those whose
                                       id contains one of the TEXTs), print the table, write
                                       the JSON
  fixtures                             make the fixtures only
  list                                 list the scenario ids
  run ID [--timeout SECS]              run one scenario here and print its samples as JSON

Build with --release: that's the reference.";

/// How long one scenario may take, setup included, before it's cut short.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    match cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "colosso-bench: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn cli() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let timeout = flag(&args, "--timeout")?
        .map(|secs| secs.parse::<u64>().map(Duration::from_secs))
        .transpose()
        .context("--timeout takes whole seconds")?
        .unwrap_or(DEFAULT_TIMEOUT);
    match args.first().map(String::as_str) {
        Some("all") => all(flag(&args, "--only")?.unwrap_or(""), timeout),
        Some("fixtures") => {
            let infos = fixtures::ensure()?;
            let mut out = std::io::stdout().lock();
            for info in infos {
                writeln!(
                    out,
                    "{:<14} {:>11} bytes {:>9} lines {:>7} files  {}",
                    info.name, info.bytes, info.lines, info.files, info.file
                )?;
            }
            Ok(())
        }
        Some("list") => {
            let mut out = std::io::stdout().lock();
            for scenario in Scenario::all() {
                writeln!(out, "{}", scenario.id())?;
            }
            Ok(())
        }
        Some("run") => {
            let id = args.get(1).context("run takes a scenario id; see `list`")?;
            let scenario = Scenario::from_id(id).with_context(|| format!("no scenario {id}"))?;
            let measured = run::run(scenario, Deadline::new(timeout))?;
            let mut out = std::io::stdout().lock();
            serde_json::to_writer(&mut out, &measured)?;
            writeln!(out)?;
            Ok(())
        }
        _ => {
            let mut out = std::io::stdout().lock();
            writeln!(out, "{USAGE}")?;
            writeln!(
                out,
                "Fixtures go to {FIXTURES_DIR}/, results to {RESULTS_DIR}/."
            )?;
            Ok(())
        }
    }
}

/// The value after `name`, if `name` was given.
fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(value) => Ok(Some(value)),
        None => bail!("{name} needs a value"),
    }
}

fn all(only: &str, timeout: Duration) -> Result<()> {
    let fixtures = fixtures::ensure()?;
    let scenarios: Vec<Scenario> = Scenario::all()
        .into_iter()
        .filter(|scenario| only.split(',').any(|text| scenario.id().contains(text)))
        .collect();
    if scenarios.is_empty() {
        bail!("no scenario id contains {only:?}; see `list`");
    }
    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let outcomes = suite::run_all(&scenarios, timeout)?;
    // The scans open no window, so leave the scale at 0.
    let scale_factor = outcomes.iter().find_map(|(_, outcome)| match outcome {
        Outcome::Measured(measured) if measured.scale_factor > 0. => Some(measured.scale_factor),
        _ => None,
    });
    let rows = report::rows(outcomes, &fixtures);
    let results = report::Results {
        timestamp,
        machine: report::machine(),
        build_profile: report::build_profile(),
        versions: report::versions(),
        window: report::window(scale_factor),
        timeout_s: timeout.as_secs(),
        fixtures,
        scenarios: rows,
    };
    let path = report::write_json(&results)?;
    let mut out = std::io::stdout().lock();
    write!(out, "{}", report::table(&results)?)?;
    writeln!(out, "\nresults: {}", path.display())?;
    Ok(())
}
