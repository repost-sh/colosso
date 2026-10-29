//! Reading one folder: its entries, sorted, each marked whether git ignores it, and the rules
//! its subfolders are read with.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::entry::{Entry, Kind, by_place};
use crate::ignores::Ignores;
use crate::paths::join_into;

/// Never listed: git's own folder (or a linked worktree's `.git` file) and the Finder's
/// folder settings.
const NEVER_LISTED: [&str; 2] = [".git", ".DS_Store"];

/// A folder to read.
#[derive(Debug)]
pub(crate) struct Job {
    pub dir: Arc<str>,
    /// Whether git ignores the folder, and so everything in it.
    pub ignored: bool,
    /// The rules from the folders above, which its own `.gitignore` goes over.
    pub above: Arc<Ignores>,
    /// Whether the folders listed under it are read again too, rather than only new ones.
    pub deep: bool,
    /// The folder as it was last read, if it was: what's the same is kept rather than made
    /// again.
    pub before: Option<Before>,
}

/// A folder as it was last read.
#[derive(Debug)]
pub(crate) struct Before {
    pub entries: Arc<[Entry]>,
    pub rules: Arc<Ignores>,
}

/// A folder as read.
#[derive(Debug)]
pub(crate) struct Read {
    pub job: Job,
    /// The rules its entries were matched with, which its subfolders are read under.
    pub rules: Arc<Ignores>,
    /// `None` if it's gone, or can't be read.
    pub entries: Option<Arc<[Entry]>>,
}

/// Reads the folder `job` names, under the checkout at `root`.
pub(crate) fn read(root: &Path, job: Job) -> Read {
    let path = root.join(&*job.dir);
    let listing = match fs::read_dir(&path) {
        Ok(listing) => listing,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::debug!("can't list {}: {error}", path.display());
            }
            return Read {
                rules: job.above.clone(),
                job,
                entries: None,
            };
        }
    };
    let mut found: Vec<(String, Kind)> = Vec::new();
    for item in listing.flatten() {
        let name = (item.file_name().into_string())
            .unwrap_or_else(|name| name.to_string_lossy().into_owned());
        if NEVER_LISTED.contains(&name.as_str()) {
            continue;
        }
        let Ok(file_type) = item.file_type() else {
            continue;
        };
        let kind = if file_type.is_dir() {
            Kind::Dir
        } else if file_type.is_symlink() {
            // Only asked where it leads, never followed.
            let to_dir = fs::metadata(item.path()).is_ok_and(|target| target.is_dir());
            Kind::Symlink { to_dir }
        } else {
            Kind::File
        };
        found.push((name, kind));
    }
    found.sort_by(|(a, a_kind), (b, b_kind)| {
        by_place((a_kind.is_folder(), a), (b_kind.is_folder(), b))
    });
    // An ignored folder's own `.gitignore` can't bring anything in it back.
    let has_gitignore =
        (found.iter()).any(|(name, kind)| name == ".gitignore" && *kind == Kind::File);
    let before = job.before.as_ref();
    let rules = if has_gitignore && !job.ignored {
        (job.above).with_gitignore(root, &job.dir, before.map(|before| &before.rules))
    } else {
        job.above.clone()
    };
    let old: &[Entry] = before.map_or(&[], |before| &before.entries);
    let mut at = 0;
    let mut path = String::new();
    let entries = found
        .into_iter()
        .map(|(name, kind)| {
            let place = (kind.is_folder(), name.as_str());
            // Both are in order, so the entry of the same name before, if there was one, is
            // the first not sorted ahead of this one.
            while (old.get(at))
                .is_some_and(|was| by_place((was.is_folder(), &was.name), place).is_lt())
            {
                at += 1;
            }
            let name: Arc<str> = match old.get(at) {
                Some(was) if *was.name == *name && was.is_folder() == place.0 => was.name.clone(),
                _ => name.into(),
            };
            // Git takes a link for a file, wherever it leads.
            let ignored = job.ignored
                || rules.ignores(join_into(&mut path, &job.dir, &name), kind == Kind::Dir);
            Entry {
                name,
                kind,
                ignored,
            }
        })
        .collect();
    Read {
        job,
        rules,
        entries: Some(entries),
    }
}
