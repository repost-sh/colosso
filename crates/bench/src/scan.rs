//! The scan scenarios: a checkout listed and followed off the main thread, on the generated
//! repository. Each sample is wall time on the bench's own thread, from the write (or the
//! open) to the update a consumer receives.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use colosso_project::{Excludes, Feed, GitDirs, Options, Project, Subscription, Update, Written};

use crate::repo::{AREAS, MODULES, PACKAGES, module};
use crate::scenarios::{Kind, Scenario};
use crate::stats::{Deadline, Measured};

/// How long any one update may take before the scenario fails.
const PATIENCE: Duration = Duration::from_secs(60);
const OPENS: usize = 3;
const TOUCHES: usize = 20;
/// The ignored burst: files written into `node_modules/`, in folders of a thousand.
const IGNORED_BURST: usize = 50_000;
/// The tracked burst: files written into existing folders, a hundred to a folder.
const TRACKED_BURST: usize = 2_000;
const TRACKED_FOLDERS: usize = 20;
const TRACKED_ROUNDS: usize = 3;
/// Where the ignored burst goes.
const IGNORED_DIR: &str = "node_modules/.bench-burst";

pub fn run(scenario: Scenario, deadline: Deadline) -> Result<Measured> {
    let root = fs::canonicalize(scenario.fixture.path())
        .context("finding the repository (run `colosso-bench fixtures` first)")?;
    let mut run = Run {
        root,
        deadline,
        measured: Measured::default(),
    };
    run.clean()?;
    match scenario.kind {
        Kind::Scan => run.scan()?,
        Kind::ScanTouch => run.touch()?,
        Kind::BurstIgnored => run.burst_ignored()?,
        Kind::BurstTracked => run.burst_tracked()?,
        _ => bail!("{} isn't a scan scenario", scenario.id()),
    }
    run.clean()?;
    Ok(run.measured)
}

struct Run {
    root: PathBuf,
    deadline: Deadline,
    measured: Measured,
}

impl Run {
    /// Takes away what an earlier run cut short may have left.
    fn clean(&self) -> Result<()> {
        let burst = self.root.join(IGNORED_DIR);
        if burst.exists() {
            fs::remove_dir_all(&burst)?;
        }
        for folder in 0..TRACKED_FOLDERS {
            remove_burst(&self.root.join(tracked_folder(folder)))?;
        }
        for marker in ["bench-touch.rs", "bench-marker"] {
            let path = self.root.join(module(0, 0, 0)).join(marker);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    /// Whether the time is up, noting when if it is.
    fn out_of_time(&mut self) -> bool {
        let passed = self.deadline.passed();
        if passed.is_some() {
            self.measured.timed_out_at_s = passed;
        }
        passed.is_some()
    }

    fn note(&mut self, note: String) {
        self.measured.notes.push(note);
    }

    /// Opens the repository and waits until every folder git doesn't ignore is listed.
    fn open(&self) -> Result<(Arc<Project>, Subscription, Duration)> {
        let started = Instant::now();
        let project = Project::open(Options {
            // The user's own excludes would make the numbers depend on whose machine it is.
            excludes: Excludes::None,
            ..Options::new(self.root.clone(), Some(GitDirs::main(&self.root)))
        });
        let updates = project.subscribe(Feed::Everything);
        until(&updates, |update| update.scanned.is_some())?;
        Ok((project, updates, started.elapsed()))
    }

    fn scan(&mut self) -> Result<()> {
        for open in 0..OPENS {
            if self.out_of_time() {
                break;
            }
            let (project, _updates, took) = self.open()?;
            self.measured.push(took);
            if open == 0 {
                let scanned = project.scanned().context("scanned, but no counts")?;
                let rss = resident_mb().map_or_else(|| "?".to_owned(), |mb| format!("{mb:.0}"));
                self.note(format!(
                    "{} folders and {} files listed; {} folders read; resident {rss} MB after it",
                    scanned.counts.folders,
                    scanned.counts.files,
                    project.stats().folders_read,
                ));
            }
        }
        Ok(())
    }

    /// A file made and removed again in turn, each timed to the listing that shows it.
    fn touch(&mut self) -> Result<()> {
        let (_project, updates, _) = self.open()?;
        let folder = module(0, 0, 0);
        let file = self.root.join(&folder).join("bench-touch.rs");
        for touch in 0..TOUCHES {
            if self.out_of_time() {
                break;
            }
            let made = touch % 2 == 0;
            let started = Instant::now();
            if made {
                fs::write(&file, "")?;
            } else {
                fs::remove_file(&file)?;
            }
            until(&updates, |update| {
                (update.listings.get(folder.as_str())).is_some_and(|listing| {
                    let has = (listing.iter().flat_map(|entries| entries.iter()))
                        .any(|entry| &*entry.name == "bench-touch.rs");
                    has == made
                })
            })?;
            self.measured.push(started.elapsed());
        }
        Ok(())
    }

    /// Fifty thousand files written into `node_modules/`, then removed: each timed as the work
    /// the project did for it, which should be next to none.
    fn burst_ignored(&mut self) -> Result<()> {
        let (project, updates, _) = self.open()?;
        let marker = format!("{}/bench-marker", module(0, 0, 0));
        // Whatever opening left to report is taken first.
        through(&updates, &self.root, &marker)?;
        for remove in [false, true] {
            if self.out_of_time() {
                break;
            }
            let before = project.stats();
            let started = Instant::now();
            if remove {
                fs::remove_dir_all(self.root.join(IGNORED_DIR))?;
            } else {
                for n in 0..IGNORED_BURST {
                    let folder = self.root.join(format!("{IGNORED_DIR}/b{}", n / 1000));
                    if n % 1000 == 0 {
                        fs::create_dir_all(&folder)?;
                    }
                    fs::write(folder.join(format!("f{n}.js")), "")?;
                }
            }
            let written = started.elapsed();
            let others = through(&updates, &self.root, &marker)?;
            let work = project.stats() - before;
            self.measured.push(work.busy);
            let what = if remove { "removed" } else { "written" };
            self.note(format!(
                "{IGNORED_BURST} files {what} in {written:.0?}: {} reports, {} dropped where they \
                 arrived; {others} updates to consumers besides the marker's; {} batches and {} \
                 folders read, the marker's included",
                work.events, work.dropped, work.batches, work.folders_read,
            ));
        }
        Ok(())
    }

    /// Two thousand files written into twenty folders, then removed, a few times over: each
    /// timed until the listings show all of it.
    fn burst_tracked(&mut self) -> Result<()> {
        let (project, updates, _) = self.open()?;
        let per_folder = TRACKED_BURST / TRACKED_FOLDERS;
        for round in 0..TRACKED_ROUNDS * 2 {
            if self.out_of_time() {
                break;
            }
            let made = round % 2 == 0;
            let before = project.stats();
            let started = Instant::now();
            for folder in 0..TRACKED_FOLDERS {
                let folder = self.root.join(tracked_folder(folder));
                if made {
                    for n in 0..per_folder {
                        fs::write(folder.join(format!("burst-{n}.rs")), "")?;
                    }
                } else {
                    remove_burst(&folder)?;
                }
            }
            let mut taken = 0;
            let mut settled: Vec<usize> = Vec::new();
            until(&updates, |update| {
                taken += 1;
                for folder in 0..TRACKED_FOLDERS {
                    let listing = update.listings.get(tracked_folder(folder).as_str());
                    let count = (listing.and_then(Option::as_ref)).map(|entries| {
                        (entries.iter())
                            .filter(|entry| entry.name.starts_with("burst-"))
                            .count()
                    });
                    let wanted = if made { per_folder } else { 0 };
                    if count == Some(wanted) && !settled.contains(&folder) {
                        settled.push(folder);
                    }
                }
                settled.len() == TRACKED_FOLDERS
            })?;
            self.measured.push(started.elapsed());
            let work = project.stats() - before;
            let what = if made { "written" } else { "removed" };
            self.note(format!(
                "{TRACKED_BURST} files {what}: settled after {taken} updates, {} batches, {} \
                 folders read, {:.1?} of work",
                work.batches, work.folders_read, work.busy,
            ));
        }
        Ok(())
    }
}

/// The folder the tracked burst writes into, by number: the last module of each area, a
/// package at a time.
fn tracked_folder(n: usize) -> String {
    module(n % PACKAGES, n / PACKAGES % AREAS, MODULES - 1)
}

fn remove_burst(folder: &Path) -> Result<()> {
    if !folder.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("burst-") {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Takes updates until `wanted` accepts one, failing past `PATIENCE`.
fn until(updates: &Subscription, wanted: impl FnMut(&Update) -> bool) -> Result<()> {
    if !updates.until(Instant::now() + PATIENCE, wanted) {
        bail!("no update in {PATIENCE:?}");
    }
    Ok(())
}

/// Writes the file `marker` under `root` (or removes it) and takes updates until its folder's
/// listing says so; how many updates came before that said anything but the marker.
fn through(updates: &Subscription, root: &Path, marker: &str) -> Result<usize> {
    let path = root.join(marker);
    let made = !path.exists();
    if made {
        fs::write(&path, "")?;
    } else {
        fs::remove_file(&path)?;
    }
    let (folder, name) = marker.rsplit_once('/').unwrap_or(("", marker));
    let mut others = 0;
    until(updates, |update| {
        let only_marker = update.listings.keys().all(|dir| &**dir == folder)
            && matches!(&update.written,
                Written::Paths(paths) if paths.iter().all(|path| &**path == marker))
            && update.git.is_empty();
        if !only_marker {
            others += 1;
        }
        let listing = updates.project().listing(folder);
        let listed =
            (listing.iter().flat_map(|entries| entries.iter())).any(|entry| &*entry.name == name);
        listed == made
    })?;
    Ok(others)
}

/// This process's resident memory, as `ps` reports it.
fn resident_mb() -> Option<f64> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    let kb: f64 = String::from_utf8(output.stdout).ok()?.trim().parse().ok()?;
    Some(kb / 1024.)
}
