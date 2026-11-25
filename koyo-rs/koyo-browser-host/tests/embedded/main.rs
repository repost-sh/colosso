//! The engine as Koyo runs it: started in this process by `embedded::start`, CEF driven from
//! the main thread's run loop by the pump while `NSApp` runs it, and a page opened in a holder
//! of its own, then stopped by `embedded::finish`. Each test has its page do one thing
//! (`inside.rs`): load and draw, sit idle on screen, have Chromium put work off, leave the
//! app's window and come back with a bubble of Chromium's up, be hidden as it leaves that window
//! and visible as it comes to it, be driven by an agent once its tab was left, say its tone, be
//! zoomed, download a file with no bubble of Chromium's opening, or play and stop with a line in
//! the log for each.
//!
//! CEF starts once in a process, and on the main thread, so this is a test binary of its own
//! that owns its main thread (`harness = false`), each test run in a process of its own. Its helpers stay up only for a
//! browser process run from an app that holds the engine (`docs/native-pages.md` 3.1), so the
//! test lays out such an app round itself in the checkout, as `script/dev-bundle` does round
//! Koyo, and runs itself from there. Its `NSApp` never comes forward, as the host's own doesn't
//! (`mac_app.rs`).
//!
//! It waits for an engine `script/browser-engine` packaged, as the smoke tests do:
//!
//! ```sh
//! script/browser-engine
//! script/cargo test -p koyo-browser-host --test embedded -- --ignored
//! ```
#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a test binary of its own says what it ran as libtest does"
)]

mod asking;
mod bubbles;
mod downloads;
mod heard;
mod inside;
mod looking;
mod pump;
mod sight;
mod window;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

/// Set for the run inside the test's app: the scratch folder it keeps its profiles in.
const INSIDE: &str = "KOYO_ENGINE_TEST_SCRATCH";

/// Set for the run inside the test's app: the test it runs.
const TEST: &str = "KOYO_ENGINE_TEST";

/// How long the run inside the app may take before it's stopped.
const PATIENCE: Duration = Duration::from_secs(120);

/// What libtest's arguments ask of the tests.
struct Asked {
    list: bool,
    chosen: Vec<&'static str>,
    filtered: usize,
    ignored: bool,
}

impl Asked {
    fn parse(mut args: impl Iterator<Item = String>) -> Self {
        let (mut list, mut ignored) = (false, false);
        let (mut filters, mut skips) = (Vec::new(), Vec::new());
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => list = true,
                "--ignored" | "--include-ignored" => ignored = true,
                "--skip" => skips.extend(args.next()),
                "--test-threads" | "--format" | "--color" | "--logfile" | "-Z" => {
                    args.next();
                }
                flag if flag.starts_with('-') => {}
                _ => filters.push(arg.clone()),
            }
        }
        let chosen: Vec<&'static str> = (inside::TESTS.iter().map(|(name, _)| *name))
            .filter(|name| {
                let wanted = filters.is_empty() || filters.iter().any(|filter| name.contains(filter.as_str()));
                wanted && !skips.iter().any(|skip| name.contains(skip.as_str()))
            })
            .collect();
        Self {
            list,
            filtered: inside::TESTS.len() - chosen.len(),
            chosen,
            ignored,
        }
    }
}

fn main() -> ExitCode {
    if let (Some(scratch), Some(test)) = (std::env::var_os(INSIDE), std::env::var(TEST).ok()) {
        return inside::run(Path::new(&scratch), &test);
    }
    let asked = Asked::parse(std::env::args().skip(1));
    if asked.list {
        for name in &asked.chosen {
            println!("{name}: test");
        }
        return ExitCode::SUCCESS;
    }
    println!("\nrunning {} tests", asked.chosen.len());
    if !asked.ignored {
        for name in &asked.chosen {
            println!("test {name} ... ignored, needs a packaged browser engine; see the module docs");
        }
        println!(
            "\ntest result: ok. 0 passed; 0 failed; {} ignored; 0 measured; {} filtered out\n",
            asked.chosen.len(),
            asked.filtered
        );
        return ExitCode::SUCCESS;
    }
    let mut failures = Vec::new();
    for name in &asked.chosen {
        let began = Instant::now();
        let ran = relaunched(name);
        let verdict = if ran.is_ok() { "ok" } else { "FAILED" };
        println!("test {name} ... {verdict} ({:.1} s)", began.elapsed().as_secs_f64());
        if let Err(why) = ran {
            failures.push((name, why));
        }
    }
    let passed = asked.chosen.len() - failures.len();
    if !failures.is_empty() {
        for (name, why) in &failures {
            println!("\n---- {name} ----\n{why}");
        }
        println!("\nfailures:");
        for (name, _) in &failures {
            println!("    {name}");
        }
        println!(
            "\ntest result: FAILED. {passed} passed; {} failed; 0 ignored; 0 measured; {} filtered out\n",
            failures.len(),
            asked.filtered
        );
        return ExitCode::FAILURE;
    }
    println!(
        "\ntest result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {} filtered out\n",
        asked.filtered
    );
    ExitCode::SUCCESS
}

/// The engine the test runs: the one `KOYO_BROWSER_ENGINE` names, else the newest
/// `script/browser-engine` packaged in the checkout.
fn engine() -> Result<PathBuf, String> {
    if let Some(folder) = std::env::var_os("KOYO_BROWSER_ENGINE") {
        return Ok(PathBuf::from(folder));
    }
    let packaged = Path::new(env!("KOYO_TARGET_DIR")).join("browser-engine");
    let mut engines: Vec<PathBuf> = std::fs::read_dir(&packaged)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    engines.retain(|folder| folder.join("engine.json").is_file());
    engines.sort();
    engines
        .pop()
        .ok_or_else(|| "no engine packaged in the checkout: run script/browser-engine".to_owned())
}

/// Runs `program` with `args` to its end, saying what failed.
fn run(program: &str, args: &[&Path]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|error| format!("{program}: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("{program} {args:?}: {status}"))
}

/// Lays out an app in the checkout that holds this test binary and the engine, as
/// `script/dev-bundle` lays out Koyo's, runs the test `name` from it and waits for it: how it
/// ended. The app goes once it passed; a failed run's stays, with the engine's log in its
/// profiles.
fn relaunched(name: &str) -> Result<(), String> {
    let engine = engine()?.join("Koyo Browser Host.app");
    let scratch = Path::new(env!("KOYO_CHECKOUT"))
        .join(".superpowers/t")
        .join(format!("{}-embedded-{name}", std::process::id()));
    let contents = scratch.join("Koyo Engine Test.app/Contents");
    let frameworks = contents.join("Frameworks");
    let executable = contents.join("MacOS/koyo-engine-test");
    for folder in [&frameworks, &contents.join("MacOS")] {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    let this = std::env::current_exe().map_err(|error| error.to_string())?;
    run("cp", &[Path::new("-c"), &this, &executable])?;
    run("cp", &[Path::new("-cR"), &engine, &frameworks])?;
    std::fs::write(contents.join("Info.plist"), PLIST).map_err(|error| error.to_string())?;
    let mut child = Command::new(&executable)
        .env(INSIDE, &scratch)
        .env(TEST, name)
        .spawn()
        .map_err(|error| format!("{}: {error}", executable.display()))?;
    let deadline = Instant::now() + PATIENCE;
    let (_never, tick) = std::sync::mpsc::channel::<()>();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "the run in the app took over {PATIENCE:?}; it's kept at {}",
                scratch.display()
            ));
        }
        let _ = tick.recv_timeout(Duration::from_millis(100));
    };
    if !status.success() {
        return Err(format!(
            "the run in the app failed ({status}); it's kept at {}",
            scratch.display()
        ));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

/// The test app's `Info.plist`: an agent, with no dock icon or menu bar, as the engine's host is.
const PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>koyo-engine-test</string>
  <key>CFBundleIdentifier</key><string>codes.koyo.desktop.engine-test</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Koyo Engine Test</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#;
