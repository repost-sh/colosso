use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::failure::Failure;
use crate::language::Language;

const TRACKED_PATHSPECS: [&str; 3] = ["koyo-rs/koyo", "koyo-rs/koyo-*", "koyo-js"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    MayBeGone,
    Required,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub display: String,
    pub path: PathBuf,
    pub language: Language,
    pub presence: Presence,
}

/// The top of the git checkout the tool runs in.
pub fn checkout_root() -> Result<PathBuf, Failure> {
    let output = git(None, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(OsStr::from_bytes(output.trim_ascii_end())))
}

pub fn listed_by_git(root: &Path) -> Result<Vec<Source>, Failure> {
    let mut arguments = vec![
        "ls-files",
        "-z",
        "-co",
        "--exclude-standard",
        "--full-name",
        "--",
    ];
    arguments.extend(TRACKED_PATHSPECS);
    let listing = git(Some(root), &arguments)?;
    let sources = listing
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
        .filter_map(|name| {
            let relative = Path::new(OsStr::from_bytes(name));
            Some(Source {
                display: relative.to_string_lossy().into_owned(),
                path: root.join(relative),
                language: Language::of_path(relative)?,
                presence: Presence::MayBeGone,
            })
        })
        .collect();
    Ok(in_path_order(sources))
}

pub fn named(paths: &[PathBuf]) -> Result<Vec<Source>, Failure> {
    let mut sources = Vec::new();
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|failure| Failure::new(path.display(), failure))?;
        if metadata.is_dir() {
            walk(path, &mut sources)?;
        } else if let Some(language) = Language::of_path(path) {
            sources.push(source(path, language));
        } else {
            return Err(Failure::new(
                path.display(),
                "not a Rust (.rs) or TypeScript (.ts, .tsx) file",
            ));
        }
    }
    Ok(in_path_order(sources))
}

fn walk(directory: &Path, sources: &mut Vec<Source>) -> Result<(), Failure> {
    let unreadable = |failure| Failure::new(directory.display(), failure);
    let entries = fs::read_dir(directory)
        .map_err(unreadable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(unreadable)?;
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().map_err(unreadable)?;
        if kind.is_dir() {
            walk(&path, sources)?;
        } else if kind.is_file()
            && let Some(language) = Language::of_path(&path)
        {
            sources.push(source(&path, language));
        }
    }
    Ok(())
}

fn source(path: &Path, language: Language) -> Source {
    Source {
        display: path.display().to_string(),
        path: path.to_owned(),
        language,
        presence: Presence::Required,
    }
}

fn in_path_order(mut sources: Vec<Source>) -> Vec<Source> {
    sources.sort_by(|left, right| left.display.cmp(&right.display));
    sources.dedup_by(|left, right| left.display == right.display);
    sources
}

pub fn git(directory: Option<&Path>, arguments: &[&str]) -> Result<Vec<u8>, Failure> {
    let output = run_git(directory, arguments)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(git_failed(arguments, &output))
    }
}

/// What git prints, or `None` when it exits 1, as `git rev-parse --verify -q` does for a name
/// that names nothing.
pub fn git_lookup(root: &Path, arguments: &[&str]) -> Result<Option<Vec<u8>>, Failure> {
    let output = run_git(Some(root), arguments)?;
    match output.status.code() {
        Some(0) => Ok(Some(output.stdout)),
        Some(1) => Ok(None),
        _ => Err(git_failed(arguments, &output)),
    }
}

fn run_git(directory: Option<&Path>, arguments: &[&str]) -> Result<Output, Failure> {
    let mut command = Command::new("git");
    if let Some(directory) = directory {
        command.arg("-C").arg(directory);
    }
    command
        .args(arguments)
        .output()
        .map_err(|failure| Failure::new("git", format!("cannot run git: {failure}")))
}

fn git_failed(arguments: &[&str], output: &Output) -> Failure {
    Failure::new(
        "git",
        format!(
            "git {} failed: {}",
            arguments.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    )
}
