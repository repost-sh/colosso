//! What a checkout's writes mean for its diff. The checkout's project watches it, once for
//! every panel showing it, and drops what's written inside ignored folders where the
//! platform reports it; what reaches here is sorted into what each write means for the diff.

use colosso_project::{Update, Written};

/// What a changed path means for the checkout's diff.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Touched {
    /// A file under the checkout, relative to it with `/` between folders.
    File(String),
    /// Something inside this folder, relative to the checkout: too much was written in it to
    /// name file by file.
    Folder(String),
    /// The index, which staged changes are read from and unstaged ones compared with.
    Index,
    /// `HEAD` or a branch: what the files are compared with may have moved.
    Head,
    /// Anything: too much was written to name it folder by folder, or the platform lost
    /// track.
    Everything,
}

/// What `update` means for the diff.
pub fn touched(update: &Update) -> Vec<Touched> {
    let mut touched: Vec<Touched> = match &update.written {
        Written::Paths(paths) => (paths.iter())
            .map(|path| Touched::File(path.to_string()))
            .collect(),
        Written::Folders(folders) => (folders.iter())
            .map(|folder| Touched::Folder(folder.to_string()))
            .collect(),
        Written::Everything => vec![Touched::Everything],
    };
    if update.git.index {
        touched.push(Touched::Index);
    }
    if update.git.head {
        touched.push(Touched::Head);
    }
    touched
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use colosso_project::{Excludes, Feed, Git, GitDirs, Options, Project, Subscription};

    use super::*;
    use crate::test_support::{git, scratch};

    #[test]
    fn writes_are_named_by_file_by_folder_or_not_at_all() {
        let update = Update {
            written: Written::Paths(["src/lib.rs".into()].into()),
            git: Git {
                index: true,
                head: false,
            },
            ..Update::default()
        };
        assert_eq!(
            touched(&update),
            [Touched::File("src/lib.rs".into()), Touched::Index]
        );
        let flood = Update {
            written: Written::Folders(["src".into()].into()),
            git: Git {
                index: false,
                head: true,
            },
            ..Update::default()
        };
        assert_eq!(
            touched(&flood),
            [Touched::Folder("src".into()), Touched::Head]
        );
        let lost = Update {
            written: Written::Everything,
            ..Update::default()
        };
        assert_eq!(touched(&lost), [Touched::Everything]);
    }

    /// A repository made for one test under the target folder.
    fn repository(name: &str) -> PathBuf {
        let root = scratch(name);
        git(&root, &["init", "-q", "-b", "main"]);
        root
    }

    /// The repository at `root` opened as the app opens it, with what it writes followed as
    /// the changes panel follows it, once the first scan is done.
    fn follow(root: &Path) -> (Arc<Project>, Subscription) {
        let project = Project::open(Options {
            excludes: Excludes::None,
            ..Options::new(root.to_owned(), Some(GitDirs::main(root)))
        });
        let written = project.subscribe(Feed::Written);
        let everything = project.subscribe(Feed::Everything);
        let scanned = reported(&everything, |update| update.scanned.is_some());
        assert!(scanned.is_some(), "the first scan never finished");
        (project, written)
    }

    /// What the watcher reported until `wanted` says it reported enough, if it did within a
    /// few seconds.
    fn reported(updates: &Subscription, wanted: impl FnMut(&Update) -> bool) -> Option<()> {
        (updates.until(Instant::now() + Duration::from_secs(10), wanted)).then_some(())
    }

    #[test]
    fn writes_staging_and_commits_are_reported() {
        let root = repository("watch");
        let (_project, written) = follow(&root);
        let until = |wanted: Touched| {
            reported(&written, |update| touched(update).contains(&wanted)).is_some()
        };
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        assert!(until(Touched::File("a.txt".into())));
        git(&root, &["add", "a.txt"]);
        assert!(until(Touched::Index));
        git(&root, &["commit", "-q", "-m", "a"]);
        assert!(until(Touched::Head));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn writes_inside_ignored_folders_are_not_reported() {
        let root = repository("ignored");
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
        let (_project, written) = follow(&root);
        // Whatever the setup left for the watcher to report is taken first.
        std::fs::write(root.join("before"), "").unwrap();
        let before = Touched::File("before".into());
        assert!(reported(&written, |update| touched(update).contains(&before)).is_some());
        for n in 0..200 {
            std::fs::write(root.join(format!("node_modules/f{n}.js")), "").unwrap();
        }
        // Reported in order: anything about the writes would come before the marker.
        std::fs::write(root.join("marker"), "").unwrap();
        let marker = Touched::File("marker".into());
        let mut seen = Vec::new();
        let reached = reported(&written, |update| {
            seen.extend(touched(update));
            seen.contains(&marker)
        });
        let _ = std::fs::remove_dir_all(&root);
        assert!(reached.is_some());
        assert_eq!(seen, [marker]);
    }
}
