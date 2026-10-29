//! A checkout made for each test under the target folder, and waiting on what its project
//! says about it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_project::{Entry, Excludes, Feed, GitDirs, Options, Project, Subscription, Update};

/// How long a test waits for the watcher before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

/// A folder of files for one test, removed when it ends.
pub struct Checkout {
    pub root: PathBuf,
}

impl Checkout {
    pub fn new(name: &str) -> Checkout {
        // The checkout's own target folder: the shared build folder cargo's temporary
        // folder is in may be outside it.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-project")
            .join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // The platform reports real paths; the tests compare with what it reports.
        Checkout {
            root: fs::canonicalize(&root).unwrap(),
        }
    }

    pub fn write(&self, path: &str, contents: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    pub fn mkdir(&self, path: &str) {
        fs::create_dir_all(self.root.join(path)).unwrap();
    }

    pub fn remove(&self, path: &str) {
        let path = self.root.join(path);
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }

    pub fn rename(&self, from: &str, to: &str) {
        fs::rename(self.root.join(from), self.root.join(to)).unwrap();
    }

    /// Options for this checkout with no rules but its own: no user excludes, and the git
    /// folder at `.git` if there is one.
    pub fn options(&self) -> Options {
        let git = (self.root.join(".git").is_dir()).then(|| GitDirs::main(&self.root));
        Options {
            excludes: Excludes::None,
            ..Options::new(self.root.clone(), git)
        }
    }

    /// Opens the checkout with `options` and waits for the first scan.
    pub fn open_with(&self, options: Options) -> (Arc<Project>, Subscription) {
        let project = Project::open(options);
        let updates = project.subscribe(Feed::Everything);
        assert!(
            until(&updates, |update| update.scanned.is_some()),
            "the first scan never finished"
        );
        (project, updates)
    }

    pub fn open(&self) -> (Arc<Project>, Subscription) {
        self.open_with(self.options())
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Takes updates until `done` says one is what was waited for; whether one was in time.
pub fn until(updates: &Subscription, done: impl FnMut(&Update) -> bool) -> bool {
    updates.until(Instant::now() + PATIENCE, done)
}

/// Takes updates until `dir`'s listing is one `wanted` accepts; whether it was in time.
pub fn until_listed(
    updates: &Subscription,
    dir: &str,
    mut wanted: impl FnMut(Option<&[Entry]>) -> bool,
) -> bool {
    if wanted(updates.project().listing(dir).as_deref()) {
        return true;
    }
    until(updates, |update| match update.listings.get(dir) {
        Some(listing) => wanted(listing.as_deref()),
        None => false,
    })
}

/// The names in a listing, in its order.
pub fn names(listing: &[Entry]) -> Vec<&str> {
    listing.iter().map(|entry| &*entry.name).collect()
}

/// The names in a listing git ignores.
pub fn ignored(listing: &[Entry]) -> Vec<&str> {
    (listing.iter())
        .filter(|entry| entry.ignored)
        .map(|entry| &*entry.name)
        .collect()
}
