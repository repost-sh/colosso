//! The results: a table for whoever ran the bench, and a JSON file for the next run to be
//! compared with.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::fixtures::{self, FixtureInfo, RESULTS_DIR};
use crate::harness::{HEIGHT, WIDTH};
use crate::scenarios::{Budget, Scenario};
use crate::stats::Summary;
use crate::suite::Outcome;

#[derive(Serialize)]
pub struct Results {
    pub timestamp: String,
    pub machine: Machine,
    /// `release` is the reference; a debug build's numbers aren't comparable.
    pub build_profile: &'static str,
    /// The GPUI crates the editor is built from, as the lockfile resolved them.
    pub versions: BTreeMap<&'static str, String>,
    pub window: Window,
    pub timeout_s: u64,
    pub fixtures: Vec<FixtureInfo>,
    pub scenarios: Vec<Row>,
}

#[derive(Serialize)]
pub struct Machine {
    pub cpu: String,
    pub cores: usize,
    pub memory_bytes: Option<u64>,
    pub os: String,
    pub arch: &'static str,
}

#[derive(Serialize)]
pub struct Window {
    pub width: f32,
    pub height: f32,
    pub scale_factor: Option<f32>,
}

#[derive(Serialize)]
pub struct Row {
    pub id: String,
    /// The kind of scenario, the first half of its id.
    pub scenario: &'static str,
    pub fixture: &'static str,
    pub samples: usize,
    #[serde(flatten)]
    pub summary: Option<Summary>,
    pub budget: Option<Budget>,
    /// `ok` or `over` against the budget, or what stopped the scenario.
    pub status: String,
    pub notes: Vec<String>,
    pub samples_ms: Vec<f64>,
}

pub fn rows(outcomes: Vec<(Scenario, Outcome)>, fixtures: &[FixtureInfo]) -> Vec<Row> {
    outcomes
        .into_iter()
        .map(|(scenario, outcome)| {
            let bytes = fixtures
                .iter()
                .find(|info| info.name == scenario.fixture.name)
                .map_or(0, |info| info.bytes);
            let budget = scenario.budget(bytes);
            let (summary, status, notes, samples_ms) = match outcome {
                Outcome::Measured(measured) => {
                    let summary = measured.summary();
                    let status = match (measured.timed_out_at_s, summary, budget) {
                        (Some(at), ..) => format!("timed out at {at:.1} s"),
                        (None, Some(summary), Some(budget)) => {
                            let met = budget.measured(&summary) < budget.ms;
                            (if met { "ok" } else { "over" }).to_owned()
                        }
                        (None, Some(_), None) => "-".to_owned(),
                        (None, None, _) => "no samples".to_owned(),
                    };
                    (summary, status, measured.notes, measured.samples_ms)
                }
                Outcome::Killed(at) => (
                    None,
                    format!("timed out at {at:.1} s"),
                    Vec::new(),
                    Vec::new(),
                ),
                Outcome::Failed(why) => (None, "failed".to_owned(), vec![why], Vec::new()),
            };
            Row {
                id: scenario.id(),
                scenario: scenario.kind.name(),
                fixture: scenario.fixture.name,
                samples: samples_ms.len(),
                summary,
                budget,
                status,
                notes,
                samples_ms,
            }
        })
        .collect()
}

pub fn table(results: &Results) -> Result<String, std::fmt::Error> {
    let mut out = String::new();
    let machine = &results.machine;
    let version = |name: &str| results.versions.get(name).map_or("?", String::as_str);
    writeln!(
        out,
        "colosso-bench · {} build · {} ({} cores, {}) · gpui-kit {} · gpui-fast {}",
        results.build_profile,
        machine.cpu,
        machine.cores,
        machine.os,
        version("gpui-kit"),
        version("gpui-fast"),
    )?;
    let scale = results
        .window
        .scale_factor
        .map_or_else(|| "?".to_owned(), |s| s.to_string());
    writeln!(
        out,
        "window {}x{} @{scale}x · main-thread ms for one input event and the full frame after it",
        results.window.width, results.window.height,
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "{:<14} {:>11} {:>9} {:>14}",
        "fixture", "bytes", "lines", "longest line"
    )?;
    for info in &results.fixtures {
        writeln!(
            out,
            "{:<14} {:>11} {:>9} {:>14}",
            info.name, info.bytes, info.lines, info.longest_line_bytes
        )?;
    }
    writeln!(out)?;
    writeln!(
        out,
        "{:<13} {:<13} {:>4} {:>9} {:>9} {:>9}  {:<12} status",
        "scenario", "fixture", "n", "p50 ms", "p95 ms", "max ms", "budget ms"
    )?;
    for row in &results.scenarios {
        let number = |pick: fn(&Summary) -> f64| {
            row.summary
                .as_ref()
                .map_or_else(|| "-".to_owned(), |s| format!("{:.2}", pick(s)))
        };
        let budget = row.budget.map_or_else(String::new, |budget| {
            format!("{} < {}", budget.stat.name(), budget.ms)
        });
        writeln!(
            out,
            "{:<13} {:<13} {:>4} {:>9} {:>9} {:>9}  {:<12} {}",
            row.scenario,
            row.fixture,
            row.samples,
            number(|s| s.p50),
            number(|s| s.p95),
            number(|s| s.max),
            budget,
            row.status,
        )?;
    }
    let notes: Vec<String> = results
        .scenarios
        .iter()
        .flat_map(|row| row.notes.iter().map(|note| format!("  {}: {note}", row.id)))
        .collect();
    if !notes.is_empty() {
        writeln!(out, "\nnotes")?;
        for note in notes {
            writeln!(out, "{note}")?;
        }
    }
    Ok(out)
}

pub fn write_json(results: &Results) -> Result<PathBuf> {
    let dir = fixtures::in_checkout(RESULTS_DIR);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{}.json", results.timestamp.replace(':', "-")));
    let json = serde_json::to_string_pretty(results)?;
    fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

pub fn window(scale_factor: Option<f32>) -> Window {
    Window {
        width: WIDTH,
        height: HEIGHT,
        scale_factor,
    }
}

pub fn machine() -> Machine {
    Machine {
        cpu: sysctl("machdep.cpu.brand_string").unwrap_or_else(|| "unknown".to_owned()),
        cores: std::thread::available_parallelism().map_or(0, std::num::NonZero::get),
        memory_bytes: sysctl("hw.memsize").and_then(|bytes| bytes.parse().ok()),
        os: command("sw_vers", &["-productVersion"]).map_or_else(
            || std::env::consts::OS.to_owned(),
            |version| format!("macOS {version}"),
        ),
        arch: std::env::consts::ARCH,
    }
}

pub fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// The GPUI crates the editor is built from.
const GPUI_CRATES: [&str; 4] = ["gpui-kit", "gpui-fast", "gpui-base", "gpui-component"];

/// The versions the lockfile resolved, which are the ones built in.
pub fn versions() -> BTreeMap<&'static str, String> {
    let lock = fs::read_to_string(fixtures::in_checkout("Cargo.lock")).unwrap_or_default();
    GPUI_CRATES
        .into_iter()
        .map(|name| {
            let header = format!("name = \"{name}\"\nversion = \"");
            let version = lock
                .find(&header)
                .and_then(|at| lock[at + header.len()..].split('"').next())
                .unwrap_or("unknown");
            (name, version.to_owned())
        })
        .collect()
}

fn sysctl(name: &str) -> Option<String> {
    command("sysctl", &["-n", name])
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (output.status.success() && !text.is_empty()).then(|| text.to_owned())
}
