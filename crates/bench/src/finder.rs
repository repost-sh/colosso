//! The finder scenarios: Go to File's search over the tree of half a million files the rows
//! scenarios work, held in memory by folder as the project hands them out. No window opens:
//! each sample is the search's own work, off the main thread in the app, up to the results
//! the next frame draws.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result, bail, ensure};
use colosso_project::{Finder, Folder, MOST_FOUND, Results};

use crate::rows::Tree;
use crate::scenarios::{Kind, Scenario};
use crate::stats::{self, Deadline, Measured};

/// Typed a letter at a time, then erased: every prefix of it matches each of `src/`'s
/// 500,000 files, so no keystroke gets off lightly.
const QUERY: &str = "modfil";
/// Times the query is typed and erased, each from a finder just opened.
const ROUNDS: usize = 20;
const UPDATES: usize = 60;
/// The folder each `finder-update` sample adds, with this many files the query matches.
const ADDED: &str = "src/pkg-000/mod-new";
const ADDED_FILES: usize = 1000;

pub fn run(scenario: Scenario, deadline: Deadline) -> Result<Measured> {
    let folders = folders(&Tree::new());
    let mut measured = Measured::default();
    let started = Instant::now();
    let finder = Finder::new();
    finder.set_files(&folders);
    let taken = started.elapsed();
    let listed = search(&finder, "")?;
    measured.notes.push(format!(
        "{} files in {} folders; taken up in {:.1} ms, first listed {:.1} ms after it",
        finder.len(),
        folders.len(),
        taken.as_secs_f64() * 1000.,
        started.elapsed().saturating_sub(taken).as_secs_f64() * 1000.,
    ));
    ensure!(listed.found.len() == MOST_FOUND, "nothing listed");
    match scenario.kind {
        Kind::FinderQuery => query(&finder, deadline, &mut measured)?,
        Kind::FinderUpdate => update(&finder, &folders, deadline, &mut measured)?,
        _ => bail!("{} isn't a finder scenario", scenario.id()),
    }
    Ok(measured)
}

/// The tree's files by folder, in no particular order, as `Project::files_by_folder` has
/// them.
fn folders(tree: &Tree) -> Vec<Folder> {
    (tree.dirs.iter())
        .filter_map(|(dir, entries)| Folder::of(dir.clone(), entries))
        .collect()
}

/// What a search for `typed` finds; nothing else searches meanwhile, so it never gives way.
fn search(finder: &Finder, typed: &str) -> Result<Results> {
    (finder.search(typed, MOST_FOUND)).with_context(|| format!("{typed:?} gave way"))
}

/// Each prefix of the query, typed on and then erased to nothing.
fn keystrokes() -> Vec<&'static str> {
    let typed = (1..=QUERY.len()).map(|end| &QUERY[..end]);
    typed
        .chain((0..QUERY.len()).rev().map(|end| &QUERY[..end]))
        .collect()
}

/// Each sample one keystroke, to the results; the finder forgets its searches before each
/// round, as it does when Go to File closes.
fn query(finder: &Finder, deadline: Deadline, measured: &mut Measured) -> Result<()> {
    let keystrokes = keystrokes();
    let mut matched = vec![0; keystrokes.len()];
    stats::sample(deadline, measured, ROUNDS * keystrokes.len(), |sample| {
        let at = sample % keystrokes.len();
        if at == 0 {
            finder.forget();
        }
        let started = Instant::now();
        let results = search(finder, keystrokes[at])?;
        let took = started.elapsed();
        ensure!(
            !results.found.is_empty(),
            "{:?} found nothing",
            keystrokes[at]
        );
        matched[at] = results.matched;
        Ok(took)
    })?;
    let matched: Vec<String> = (keystrokes.iter().zip(&matched))
        .map(|(typed, matched)| format!("{typed:?} {matched}"))
        .collect();
    measured
        .notes
        .push(format!("matched, by keystroke: {}", matched.join(", ")));
    Ok(())
}

/// Each sample a folder of `ADDED_FILES` files taken up while the query is typed, to the
/// results with them; the folder is taken away again between samples, untimed.
fn update(
    finder: &Finder,
    folders: &[Folder],
    deadline: Deadline,
    measured: &mut Measured,
) -> Result<()> {
    for typed in &keystrokes()[..QUERY.len()] {
        search(finder, typed)?;
    }
    let before = search(finder, QUERY)?.matched;
    let added = Folder {
        path: ADDED.into(),
        files: (0..ADDED_FILES)
            .map(|file| Arc::from(format!("{ADDED}/file-{file:03}.rs")))
            .collect(),
    };
    let with: Vec<Folder> = folders.iter().cloned().chain([added]).collect();
    stats::sample(deadline, measured, UPDATES, |_| {
        finder.set_files(folders);
        search(finder, QUERY)?;
        let started = Instant::now();
        ensure!(finder.set_files(&with), "the folder added changed nothing");
        let results = search(finder, QUERY)?;
        let took = started.elapsed();
        ensure!(
            results.matched == before + ADDED_FILES,
            "{} matched with the folder added, not {}",
            results.matched,
            before + ADDED_FILES
        );
        Ok(took)
    })?;
    measured.notes.push(format!(
        "{ADDED_FILES} files added to {} while {QUERY:?} is typed, {before} matched before",
        finder.len() - ADDED_FILES
    ));
    Ok(())
}
