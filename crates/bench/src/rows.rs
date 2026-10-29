//! The rows scenarios: the file tree's rows opened, closed, listed anew and revealed, on a
//! tree of half a million files held in memory. Nothing is read from disk and no window
//! opens: each sample is the rows model's own work, which the frame after it waits on.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, bail, ensure};
use colosso_project::{Entry, Kind as EntryKind, Listings, Reveal, Row, Rows, order};

use crate::scenarios::{Kind, Scenario};
use crate::stats::{self, Deadline, Measured};

/// `big/`, one folder of this many files.
const WIDE: usize = 10_000;
/// `deep/`, folders this deep with one file at the bottom.
const DEPTH: usize = 24;
/// `src/`: packages of modules of files.
const PACKAGES: usize = 100;
const MODULES: usize = 50;
const FILES: usize = 100;
/// Every file in the tree.
pub const TREE_FILES: usize = WIDE + 1 + PACKAGES * MODULES * FILES + 2;
const SAMPLES: usize = 60;
/// The folder each `rows-update` sample adds a file to and takes it away from again: the
/// first module, so every row of the rest of the tree comes after it.
const UPDATED: &str = "src/pkg-000/mod-00";

/// The tree's listings, by folder.
struct Tree {
    dirs: HashMap<Arc<str>, Arc<[Entry]>>,
}

impl Listings for Tree {
    fn listing(&self, dir: &str) -> Option<Arc<[Entry]>> {
        self.dirs.get(dir).cloned()
    }

    /// Nothing in it is ignored.
    fn load(&self, _: &str) {}
}

impl Tree {
    fn new() -> Tree {
        let mut tree = Tree {
            dirs: HashMap::new(),
        };
        tree.put("", &["big/", "deep/", "src/", "Cargo.toml", "README.md"]);
        let big: Vec<String> = (0..WIDE).map(|n| format!("item-{n:05}.rs")).collect();
        tree.put("big", &big);
        let mut dir = "deep".to_owned();
        for level in 0..DEPTH {
            tree.put(&dir, &[format!("d{level}/")]);
            dir = format!("{dir}/d{level}");
        }
        tree.put(&dir, &["leaf.rs"]);
        let packages: Vec<String> = (0..PACKAGES).map(|n| format!("pkg-{n:03}/")).collect();
        tree.put("src", &packages);
        let modules: Vec<String> = (0..MODULES).map(|n| format!("mod-{n:02}/")).collect();
        let files: Vec<String> = (0..FILES).map(|n| format!("file-{n:03}.rs")).collect();
        for package in 0..PACKAGES {
            let package = format!("src/pkg-{package:03}");
            tree.put(&package, &modules);
            for module in 0..MODULES {
                tree.put(&format!("{package}/mod-{module:02}"), &files);
            }
        }
        tree
    }

    /// Lists `names` in `dir`, in the project's order: a name ending in `/` is a folder.
    fn put(&mut self, dir: &str, names: &[impl AsRef<str>]) -> Arc<[Entry]> {
        let mut entries: Vec<Entry> = (names.iter())
            .map(|name| {
                let name = name.as_ref();
                let kind = if name.ends_with('/') {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                };
                Entry {
                    name: name.trim_end_matches('/').into(),
                    kind,
                    ignored: false,
                }
            })
            .collect();
        entries.sort_by(order);
        let entries: Arc<[Entry]> = entries.into();
        self.dirs.insert(dir.into(), entries.clone());
        entries
    }

    /// The path of the file at the bottom of `deep/`, and the folders above it, top first.
    fn deep() -> (String, Vec<String>) {
        let mut folders = vec!["deep".to_owned()];
        for level in 0..DEPTH {
            let above = folders.last().cloned().unwrap_or_default();
            folders.push(format!("{above}/d{level}"));
        }
        let leaf = format!("{}/leaf.rs", folders.last().cloned().unwrap_or_default());
        (leaf, folders)
    }
}

pub fn run(scenario: Scenario, deadline: Deadline) -> Result<Measured> {
    let tree = Tree::new();
    let mut rows = Rows::default();
    rows.reset(&tree);
    // Every folder of `src/` open: the big tree the other folders are opened, closed and
    // listed anew in.
    rows.expand("src", &tree);
    for package in 0..PACKAGES {
        let package = format!("src/pkg-{package:03}");
        rows.expand(&package, &tree);
        for module in 0..MODULES {
            rows.expand(&format!("{package}/mod-{module:02}"), &tree);
        }
    }
    let open = 5 + PACKAGES + PACKAGES * MODULES * (1 + FILES);
    ensure!(rows.len() == open, "{} rows open, not {open}", rows.len());
    let mut run = Run {
        tree,
        rows,
        deadline,
        measured: Measured::default(),
    };
    run.note(format!(
        "{TREE_FILES} files; {open} rows shown with src/ open; {} bytes a row",
        size_of::<Row>()
    ));
    match scenario.kind {
        Kind::RowsExpand => run.sample(|rows, tree| {
            let started = Instant::now();
            ensure!(rows.expand("big", tree), "big/ didn't open");
            let took = started.elapsed();
            rows.collapse("big");
            Ok(took)
        })?,
        Kind::RowsCollapse => run.sample(|rows, tree| {
            rows.expand("big", tree);
            let started = Instant::now();
            ensure!(rows.collapse("big"), "big/ didn't close");
            Ok(started.elapsed())
        })?,
        Kind::RowsUpdate => run.update()?,
        Kind::RowsReveal => run.reveal()?,
        _ => bail!("{} isn't a rows scenario", scenario.id()),
    }
    Ok(run.measured)
}

struct Run {
    tree: Tree,
    rows: Rows,
    deadline: Deadline,
    measured: Measured,
}

impl Run {
    fn note(&mut self, note: String) {
        self.measured.notes.push(note);
    }

    /// Takes `SAMPLES` samples of `sample`, which times its own part, stopping early when
    /// the time runs out.
    fn sample(
        &mut self,
        mut sample: impl FnMut(&mut Rows, &mut Tree) -> Result<std::time::Duration>,
    ) -> Result<()> {
        let (rows, tree) = (&mut self.rows, &mut self.tree);
        stats::sample(self.deadline, &mut self.measured, SAMPLES, |_| {
            sample(rows, tree)
        })
    }

    /// A file added to the first module, then taken away again, each listing applied as an
    /// update brings it.
    fn update(&mut self) -> Result<()> {
        let files: Vec<String> = (0..FILES).map(|n| format!("file-{n:03}.rs")).collect();
        let mut more = files.clone();
        more.push("new.rs".to_owned());
        let mut added = false;
        self.sample(|rows, tree| {
            added = !added;
            let names = if added { &more } else { &files };
            let listing = tree.put(UPDATED, names);
            let changed = HashMap::from([(Arc::from(UPDATED), Some(listing))]);
            let started = Instant::now();
            ensure!(rows.apply(&changed, tree), "the update changed nothing");
            Ok(started.elapsed())
        })?;
        let shown = self.rows.position(&format!("{UPDATED}/new.rs")).is_some();
        ensure!(shown == added, "the last update isn't shown");
        Ok(())
    }

    /// The file at the bottom of `deep/` revealed with every folder above it closed.
    fn reveal(&mut self) -> Result<()> {
        let (leaf, folders) = Tree::deep();
        self.sample(|rows, tree| {
            let started = Instant::now();
            let at = rows.reveal(&leaf, tree);
            let took = started.elapsed();
            ensure!(
                matches!(at, Reveal::Shown(_)),
                "the file wasn't shown: {at:?}"
            );
            // Closed from the bottom up, so each is shown when it's closed and none is
            // left open for the next reveal to find.
            for folder in folders.iter().rev() {
                rows.collapse(folder);
            }
            Ok(took)
        })
    }
}
