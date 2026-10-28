//! Watching a checkout for what an agent, a terminal or git changes in it. The platform's
//! watcher reports paths as they change; they're sorted here into what each means for the diff.

use std::path::{Path, PathBuf};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use notify::{RecursiveMode, Watcher as _};

/// What a changed path means for the checkout's diff.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Touched {
    /// A file under the checkout, relative to it with `/` between folders.
    File(String),
    /// The index, which staged changes are read from and unstaged ones compared with.
    Index,
    /// `HEAD` or a branch: what the files are compared with may have moved.
    Head,
}

/// The folders a checkout's changes depend on.
#[derive(Clone, Debug)]
pub struct Watched {
    pub root: PathBuf,
    /// The checkout's own git folder.
    pub git_dir: PathBuf,
    /// The repository's, with the branches.
    pub common_dir: PathBuf,
}

/// A watcher and the paths it reports, or why the platform couldn't watch.
pub type Watching = notify::Result<(Watcher, UnboundedReceiver<Vec<Touched>>)>;

/// Keeps the platform's watcher running until dropped.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// Starts watching `watched`, returning the watcher and the paths it reports, already sorted.
pub fn watch(watched: Watched) -> Watching {
    // The platform reports real paths, so a folder reached through a symlink is named as it.
    let real = |path: PathBuf| std::fs::canonicalize(&path).unwrap_or(path);
    let watched = Watched {
        root: real(watched.root),
        git_dir: real(watched.git_dir),
        common_dir: real(watched.common_dir),
    };
    let (sender, receiver): (UnboundedSender<Vec<Touched>>, _) = unbounded();
    let sorting = watched.clone();
    let mut notifier = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else {
            return;
        };
        let touched: Vec<Touched> = event
            .paths
            .iter()
            .filter_map(|path| sorting.touched(path))
            .collect();
        if !touched.is_empty() {
            let _ = sender.unbounded_send(touched);
        }
    })?;
    notifier.watch(&watched.root, RecursiveMode::Recursive)?;
    // A linked worktree's git folders are outside it; a main checkout's are inside, watched
    // already.
    for dir in [&watched.git_dir, &watched.common_dir] {
        if !dir.starts_with(&watched.root) {
            notifier.watch(dir, RecursiveMode::Recursive)?;
        }
    }
    Ok((Watcher { _watcher: notifier }, receiver))
}

impl Watched {
    /// What `path` changing means, or `None` if nothing shown depends on it.
    fn touched(&self, path: &Path) -> Option<Touched> {
        for dir in [&self.git_dir, &self.common_dir] {
            if let Ok(inside) = path.strip_prefix(dir) {
                return git_file(inside);
            }
        }
        let relative = path.strip_prefix(&self.root).ok()?;
        // Another checkout's git folder, or a nested repository's: not this checkout's files.
        if relative.components().any(|part| part.as_os_str() == ".git") {
            return None;
        }
        let relative = relative.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
        (!relative.is_empty()).then_some(Touched::File(relative))
    }
}

/// What a path inside a git folder changing means.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "git names its lock files `.lock` exactly; a branch named `x.LOCK` is a branch"
)]
fn git_file(inside: &Path) -> Option<Touched> {
    let inside = inside.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
    match inside.as_str() {
        "index" => Some(Touched::Index),
        "HEAD" | "packed-refs" => Some(Touched::Head),
        // The lock git writes the new ref into first is renamed over it, which is reported too.
        refs if refs.starts_with("refs/heads/") && !refs.ends_with(".lock") => Some(Touched::Head),
        // Objects, logs, the lock files, hooks and the like change nothing shown.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked() -> Watched {
        Watched {
            root: "/work/app/feat".into(),
            git_dir: "/src/app/.git/worktrees/feat".into(),
            common_dir: "/src/app/.git".into(),
        }
    }

    #[test]
    fn files_under_the_checkout_are_named_relative_to_it() {
        let watched = linked();
        assert_eq!(
            watched.touched(Path::new("/work/app/feat/src/lib.rs")),
            Some(Touched::File("src/lib.rs".into()))
        );
        assert_eq!(watched.touched(Path::new("/work/app/feat")), None);
        assert_eq!(watched.touched(Path::new("/elsewhere/x.rs")), None);
    }

    #[test]
    fn the_index_head_and_branches_say_what_they_move() {
        let watched = linked();
        let touched = |path: &str| watched.touched(Path::new(path));
        assert_eq!(
            touched("/src/app/.git/worktrees/feat/index"),
            Some(Touched::Index)
        );
        assert_eq!(
            touched("/src/app/.git/worktrees/feat/HEAD"),
            Some(Touched::Head)
        );
        assert_eq!(
            touched("/src/app/.git/refs/heads/me/feat"),
            Some(Touched::Head)
        );
        assert_eq!(touched("/src/app/.git/refs/heads/me/feat.lock"), None);
        assert_eq!(touched("/src/app/.git/objects/ab/cdef"), None);
        assert_eq!(touched("/src/app/.git/worktrees/feat/logs/HEAD"), None);
    }

    #[test]
    fn a_main_checkouts_git_folder_is_not_one_of_its_files() {
        let watched = Watched {
            root: "/src/app".into(),
            git_dir: "/src/app/.git".into(),
            common_dir: "/src/app/.git".into(),
        };
        assert_eq!(
            watched.touched(Path::new("/src/app/.git/index")),
            Some(Touched::Index)
        );
        assert_eq!(watched.touched(Path::new("/src/app/.git/objects/x")), None);
        assert_eq!(
            watched.touched(Path::new("/src/app/vendor/dep/.git/HEAD")),
            None
        );
    }

    /// Waits up to a few seconds for the watcher to report `wanted`.
    #[expect(
        clippy::disallowed_methods,
        reason = "a synchronous test, with no runtime to wait on but the thread"
    )]
    fn reported(touched: &mut UnboundedReceiver<Vec<Touched>>, wanted: &Touched) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match touched.try_recv() {
                Ok(batch) if batch.contains(wanted) => return true,
                Ok(_) => {}
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        false
    }

    #[test]
    fn writes_staging_and_commits_are_reported() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-repos")
            .join(format!("watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
                .args([
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                ])
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        let (_watcher, mut touched) = watch(Watched {
            root: root.clone(),
            git_dir: root.join(".git"),
            common_dir: root.join(".git"),
        })
        .unwrap();
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        assert!(reported(&mut touched, &Touched::File("a.txt".into())));
        git(&["add", "a.txt"]);
        assert!(reported(&mut touched, &Touched::Index));
        git(&["commit", "-q", "-m", "a"]);
        assert!(reported(&mut touched, &Touched::Head));
        let _ = std::fs::remove_dir_all(&root);
    }
}
