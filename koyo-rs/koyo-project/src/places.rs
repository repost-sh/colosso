//! Where one checkout is, as the platform names it: its root, and its git folders when
//! they're elsewhere, as a linked worktree's are. What the platform reports is sorted here as
//! it arrives, before anything waits on it: writes inside ignored folders nobody reads, and
//! git's own files that nothing shows, go no further.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::GitDirs;
use crate::feed::Git;
use crate::paths::name;
use crate::shared::Seen;
use crate::tree::Tree;

#[derive(Debug)]
pub(crate) struct Places {
    root: String,
    /// The checkout's own git folder, then the repository's: a linked worktree's own is
    /// inside the repository's, and the nearer one names a path.
    git: Vec<String>,
}

impl Places {
    pub(crate) fn new(root: &Path, git: Option<&GitDirs>) -> Places {
        // The platform reports real paths, so a folder reached through a link is named as it.
        let real = |path: &Path| {
            let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
            text(&path).into_owned()
        };
        Places {
            root: real(root),
            git: git.map_or_else(Vec::new, |git| {
                vec![real(&git.git_dir), real(&git.common_dir)]
            }),
        }
    }

    /// The folders to watch for this checkout.
    pub(crate) fn folders(&self) -> impl Iterator<Item = PathBuf> + '_ {
        std::iter::once(&self.root)
            .chain(&self.git)
            .map(PathBuf::from)
    }

    /// Whether a report at `path` is about this checkout: it's inside the checkout or its
    /// git folders, or, when the platform `lost` track of what changed, it's above them or
    /// nowhere in particular (`""`).
    pub(crate) fn hears(&self, path: &str, lost: bool) -> bool {
        let mut places = std::iter::once(&self.root).chain(&self.git);
        places.any(|place| {
            inside(place, path).is_some()
                || lost && (path.is_empty() || inside(path, place).is_some())
        })
    }

    /// What `path` changing means, or `None` if nothing listed or read depends on it.
    pub(crate) fn seen(&self, tree: &Tree, path: &str) -> Option<Seen> {
        for dir in &self.git {
            if let Some(inside) = inside(dir, path) {
                return git_path(inside);
            }
        }
        let relative = inside(&self.root, path)?;
        // Another checkout's git folder, or a nested repository's: not this checkout's files.
        if relative.is_empty() || relative.split('/').any(|part| part == ".git") {
            return None;
        }
        if name(relative) == ".DS_Store" {
            return None;
        }
        tree.admits(relative).then(|| Seen::Path(relative.into()))
    }

    /// What to read again when the platform says it lost track of what changed at `path`
    /// and under it: that folder, unless git ignores it and nobody reads it; for a git
    /// folder, what consumers read there; for the root or above it, everything.
    pub(crate) fn rescan(&self, tree: &Tree, path: &str) -> Option<Seen> {
        if self.git.iter().any(|dir| inside(dir, path).is_some()) {
            return Some(Seen::Git(Git::ALL));
        }
        match inside(&self.root, path) {
            Some("") => Some(Seen::Everything),
            Some(relative) => {
                let unread = relative.split('/').any(|part| part == ".git")
                    || !tree.dirs.contains_key(relative) && tree.ignores(relative);
                (!unread).then(|| Seen::Rescan(relative.into()))
            }
            None if path.is_empty() || inside(path, &self.root).is_some() => Some(Seen::Everything),
            // Above a git folder elsewhere, and not the checkout.
            None => Some(Seen::Git(Git::ALL)),
        }
    }
}

/// `path` as text with `/` between its parts, as the platform's reports are compared with.
pub(crate) fn text(path: &Path) -> Cow<'_, str> {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        text
    }
}

/// `path` relative to `dir`, `""` for `dir` itself, if it's inside it.
fn inside<'a>(dir: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(dir.trim_end_matches('/'))?;
    if rest.is_empty() {
        Some(rest)
    } else {
        rest.strip_prefix('/')
    }
}

/// What a path inside a git folder changing means: only what consumers read is kept.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "git names its lock files `.lock` exactly; a branch named `x.LOCK` is a branch"
)]
fn git_path(inside: &str) -> Option<Seen> {
    let git = |index, head| Some(Seen::Git(Git { index, head }));
    match inside {
        "index" => git(true, false),
        "HEAD" | "packed-refs" => git(false, true),
        "info/exclude" => Some(Seen::Everything),
        // The lock git writes the new ref into first is renamed over it, which is reported too.
        refs if refs.starts_with("refs/heads/") && !refs.ends_with(".lock") => git(false, true),
        // Objects, logs, the lock files, hooks and the like change nothing shown.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::entry::{Entry, Kind};
    use crate::ignores::Ignores;
    use crate::tree::Dir;

    fn linked() -> Places {
        Places {
            root: "/work/app/feat".into(),
            git: vec![
                "/src/app/.git/worktrees/feat".into(),
                "/src/app/.git".into(),
            ],
        }
    }

    fn named(seen: Option<Seen>) -> Option<String> {
        Some(match seen? {
            Seen::Path(path) => format!("path {path}"),
            Seen::Git(Git { index, head }) => format!("git index {index} head {head}"),
            Seen::Rescan(dir) => format!("rescan {dir}"),
            Seen::Everything => "everything".to_owned(),
        })
    }

    fn seen(places: &Places, path: &str) -> Option<String> {
        named(places.seen(&Tree::default(), path))
    }

    #[test]
    fn a_lost_track_rescans_the_folder_named_unless_nobody_reads_it() {
        let places = linked();
        let rules = Ignores::excludes(Path::new("/work/app/feat"), &[]);
        let mut tree = Tree::default();
        let node_modules = Entry {
            name: "node_modules".into(),
            kind: Kind::Dir,
            ignored: true,
        };
        let root = Dir {
            entries: Arc::from([node_modules]),
            ignored: false,
            rules,
        };
        tree.put("".into(), root);
        let rescan = |path: &str| named(places.rescan(&tree, path));
        assert_eq!(
            rescan("/work/app/feat/src/a").as_deref(),
            Some("rescan src/a")
        );
        assert_eq!(rescan("/work/app/feat").as_deref(), Some("everything"));
        assert_eq!(rescan("/").as_deref(), Some("everything"));
        assert_eq!(rescan("").as_deref(), Some("everything"));
        assert_eq!(rescan("/work/app/feat/node_modules"), None);
        assert_eq!(rescan("/work/app/feat/node_modules/react/lib"), None);
        let all = Some("git index true head true");
        assert_eq!(rescan("/src/app/.git/refs/heads").as_deref(), all);
        assert_eq!(rescan("/src/app").as_deref(), all);
    }

    #[test]
    fn a_checkout_hears_only_what_is_about_it() {
        let places = linked();
        assert!(places.hears("/work/app/feat/src/a.rs", false));
        assert!(places.hears("/work/app/feat", false));
        assert!(places.hears("/src/app/.git/index", false));
        assert!(!places.hears("/work/app/feature/a.rs", false));
        assert!(!places.hears("/work/app", false));
        assert!(places.hears("/work/app", true));
        assert!(places.hears("", true));
        assert!(!places.hears("/work/other", true));
    }

    #[test]
    fn files_under_the_checkout_are_named_relative_to_it() {
        let places = linked();
        assert_eq!(
            seen(&places, "/work/app/feat/src/lib.rs").as_deref(),
            Some("path src/lib.rs")
        );
        assert_eq!(seen(&places, "/work/app/feat"), None);
        assert_eq!(seen(&places, "/elsewhere/x.rs"), None);
        assert_eq!(seen(&places, "/work/app/feature/x.rs"), None);
        assert_eq!(seen(&places, "/work/app/feat/src/.DS_Store"), None);
        assert_eq!(seen(&places, "/work/app/feat/.git"), None);
    }

    #[test]
    fn only_the_git_files_consumers_read_are_kept() {
        let places = linked();
        let git = |path: &str| seen(&places, path);
        let index = Some("git index true head false");
        let head = Some("git index false head true");
        assert_eq!(git("/src/app/.git/worktrees/feat/index").as_deref(), index);
        assert_eq!(git("/src/app/.git/worktrees/feat/HEAD").as_deref(), head);
        assert_eq!(git("/src/app/.git/refs/heads/me/feat").as_deref(), head);
        assert_eq!(git("/src/app/.git/packed-refs").as_deref(), head);
        assert_eq!(
            git("/src/app/.git/info/exclude").as_deref(),
            Some("everything")
        );
        assert_eq!(git("/src/app/.git/refs/heads/me/feat.lock"), None);
        assert_eq!(git("/src/app/.git/objects/ab/cdef"), None);
        assert_eq!(git("/src/app/.git/worktrees/feat/logs/HEAD"), None);
        assert_eq!(git("/src/app/.git/worktrees/other/index"), None);
    }

    #[test]
    fn a_main_checkouts_git_folder_is_not_one_of_its_files() {
        let places = Places {
            root: "/src/app".into(),
            git: vec!["/src/app/.git".into(), "/src/app/.git".into()],
        };
        assert_eq!(
            seen(&places, "/src/app/.git/index").as_deref(),
            Some("git index true head false")
        );
        assert_eq!(seen(&places, "/src/app/.git/objects/x"), None);
        assert_eq!(seen(&places, "/src/app/vendor/dep/.git/HEAD"), None);
    }
}
