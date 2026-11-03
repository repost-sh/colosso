//! Every scenario, each in a process of its own: a fresh app with cold caches per scenario,
//! one scenario's leftovers never slow the next, and a frame that never ends is cut off by
//! killing its process rather than hanging the run.

use std::io::{Read, Write as _};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

use crate::scenarios::Scenario;
use crate::stats::Measured;

/// Past the scenario's own limit, how long a run gets to notice it and report before it's
/// killed: one frame can't be interrupted, so a run stuck in one is ended from outside.
const GRACE: Duration = Duration::from_secs(15);

/// What came back from a scenario's process.
pub enum Outcome {
    Measured(Measured),
    /// Killed at this many seconds, still inside a frame or its setup.
    Killed(f64),
    Failed(String),
}

pub fn run_all(scenarios: &[Scenario], timeout: Duration) -> Result<Vec<(Scenario, Outcome)>> {
    let exe = std::env::current_exe().context("finding this program to run each scenario")?;
    let mut results = Vec::with_capacity(scenarios.len());
    for (i, &scenario) in scenarios.iter().enumerate() {
        let started = Instant::now();
        progress(&format!(
            "[{}/{}] {} ...",
            i + 1,
            scenarios.len(),
            scenario.id()
        ))?;
        let child = Command::new(&exe)
            .args([
                "run",
                &scenario.id(),
                "--timeout",
                &timeout.as_secs().to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("starting a scenario run")?;
        let outcome = wait(child, timeout + GRACE)?;
        let status = match &outcome {
            Outcome::Measured(measured) => format!("{} samples", measured.samples_ms.len()),
            Outcome::Killed(at) => format!("killed at {at:.1} s"),
            Outcome::Failed(why) => format!("failed: {why}"),
        };
        progress(&format!(
            "      {status} in {:.1} s",
            started.elapsed().as_secs_f64()
        ))?;
        results.push((scenario, outcome));
    }
    Ok(results)
}

/// Progress goes to stderr, leaving stdout to the table.
fn progress(line: &str) -> Result<()> {
    writeln!(std::io::stderr().lock(), "{line}")?;
    Ok(())
}

fn wait(mut child: Child, limit: Duration) -> Result<Outcome> {
    let started = Instant::now();
    // Stderr is drained as the run goes, so a full pipe never stalls it.
    let stderr = child.stderr.take();
    let stderr = thread::spawn(move || read_all(stderr));
    // Stdout closes when the run exits, which is what the wait is for.
    let (sender, receiver) = mpsc::channel();
    let stdout = child.stdout.take();
    thread::spawn(move || sender.send(read_all(stdout)));
    let Ok(stdout) = receiver.recv_timeout(limit) else {
        child
            .kill()
            .context("ending a scenario run past its time")?;
        child.wait()?;
        return Ok(Outcome::Killed(started.elapsed().as_secs_f64()));
    };
    let status = child.wait()?;
    if !status.success() {
        let stderr = stderr.join().unwrap_or_default();
        let lines: Vec<&str> = stderr.lines().collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        return Ok(Outcome::Failed(format!("{status}: {tail}")));
    }
    let Some(line) = stdout.lines().rev().find(|line| line.starts_with('{')) else {
        return Ok(Outcome::Failed("the run reported nothing".to_owned()));
    };
    Ok(serde_json::from_str(line).map_or_else(
        |error| Outcome::Failed(format!("unreadable report: {error}")),
        Outcome::Measured,
    ))
}

fn read_all(pipe: Option<impl Read>) -> String {
    let mut text = String::new();
    if let Some(mut pipe) = pipe {
        // A read error leaves what was read, which is all a report can be anyway.
        let _ = pipe.read_to_string(&mut text);
    }
    text
}
