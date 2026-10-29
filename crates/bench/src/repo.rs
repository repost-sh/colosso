//! The repository the scan scenarios read: a git checkout of nested packages, with as many
//! files again in `node_modules/` and `target/`, which it ignores. Made once and kept; every
//! name is drawn from a seeded generator, so it's the same tree on every machine.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

use crate::fixtures::{fnv1a, write_atomically};
use crate::generate::Rng;

/// What the scan scenarios need to know of the tree, kept beside it once it's whole.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Made {
    /// Files git doesn't ignore, the `.gitignore` included.
    pub tracked: usize,
    /// Files in `node_modules/` and `target/`.
    pub ignored: usize,
    /// FNV-1a of every path, in the order they were made.
    pub hash: String,
}

/// The folders tracked files are in: packages, their areas and the modules in each.
pub const PACKAGES: usize = 30;
pub const AREAS: usize = 10;
pub const MODULES: usize = 10;

/// The ignored halves: dependencies with a few folders each, and build outputs.
const DEPENDENCIES: usize = 500;
const DEPENDENCY_FOLDERS: usize = 3;
const CRATES: usize = 500;

const WORDS: [&str; 12] = [
    "index", "util", "main", "config", "types", "service", "handler", "model", "view", "test",
    "Store", "README",
];
const EXTENSIONS: [&str; 6] = ["rs", "ts", "tsx", "js", "json", "md"];

/// A tracked folder of the tree, by its package, area and module.
pub fn module(package: usize, area: usize, module: usize) -> String {
    format!("packages/pkg-{package:02}/src/area{area}/mod{module}")
}

/// Makes the tree at `dir` with about `tracked` and `ignored` files, unless it's there whole
/// already, and says what's in it.
pub fn ensure(dir: &Path, tracked: usize, ignored: usize, seed: u64) -> Result<Made> {
    let stamp = dir.with_extension("json");
    if dir.is_dir()
        && let Ok(text) = fs::read_to_string(&stamp)
    {
        return Ok(serde_json::from_str(&text)?);
    }
    // A tree cut short is made again from the start.
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    let made = make(dir, tracked, ignored, seed)?;
    write_atomically(&stamp, &serde_json::to_string_pretty(&made)?)?;
    Ok(made)
}

fn make(dir: &Path, tracked: usize, ignored: usize, seed: u64) -> Result<Made> {
    let per_module = tracked.div_ceil(PACKAGES * AREAS * MODULES);
    let per_dependency = (ignored / 2).div_ceil(DEPENDENCIES * DEPENDENCY_FOLDERS);
    let per_crate = (ignored / 2).div_ceil(CRATES);
    let mut rng = Rng::new(seed);
    // Every path first, so the hash says what's made before anything is written.
    let mut folders: Vec<(String, Vec<String>)> = Vec::new();
    for package in 0..PACKAGES {
        for area in 0..AREAS {
            for module_ in 0..MODULES {
                let names = (0..per_module).map(|n| name(&mut rng, n)).collect();
                folders.push((module(package, area, module_), names));
            }
        }
    }
    let tracked_files = folders.iter().map(|(_, names)| names.len()).sum::<usize>() + 1;
    for dependency in 0..DEPENDENCIES {
        for folder in 0..DEPENDENCY_FOLDERS {
            let names = (0..per_dependency)
                .map(|n| format!("chunk{n}.js"))
                .collect();
            folders.push((
                format!("node_modules/dep-{dependency:03}/lib/part{folder}"),
                names,
            ));
        }
    }
    for crate_ in 0..CRATES {
        let names = (0..per_crate).map(|n| format!("unit{n}.o")).collect();
        folders.push((format!("target/debug/build/crate-{crate_:03}/out"), names));
    }
    let all_files: usize = folders.iter().map(|(_, names)| names.len()).sum::<usize>() + 1;
    let paths: String = (folders.iter())
        .flat_map(|(folder, names)| names.iter().map(move |name| format!("{folder}/{name}\n")))
        .collect();
    let hash = fnv1a(paths.as_bytes());
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    fs::write(dir.join(".gitignore"), "node_modules/\ntarget/\n")?;
    write_all(dir, &folders)?;
    let status = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .status()
        .context("running git init")?;
    if !status.success() {
        bail!("git init in {} failed: {status}", dir.display());
    }
    Ok(Made {
        tracked: tracked_files,
        ignored: all_files - tracked_files,
        hash: format!("{hash:016x}"),
    })
}

/// A file name as a project has them: a word, a number and an extension.
fn name(rng: &mut Rng, n: usize) -> String {
    let word = rng.pick(&WORDS);
    let extension = rng.pick(&EXTENSIONS);
    format!("{word}{n}.{extension}")
}

/// Writes every folder's files, empty, on a few threads: the scan reads names, not contents.
fn write_all(dir: &Path, folders: &[(String, Vec<String>)]) -> Result<()> {
    let threads = thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = folders.len().div_ceil(threads);
    thread::scope(|scope| {
        let writers: Vec<_> = folders
            .chunks(chunk.max(1))
            .map(|share| {
                scope.spawn(move || -> Result<()> {
                    for (folder, names) in share {
                        let folder = dir.join(folder);
                        fs::create_dir_all(&folder)
                            .with_context(|| format!("creating {}", folder.display()))?;
                        for name in names {
                            fs::File::create(folder.join(name))?;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        writers
            .into_iter()
            .try_for_each(|writer| match writer.join() {
                Ok(written) => written,
                Err(_) => bail!("a thread writing the repository stopped"),
            })
    })
}
