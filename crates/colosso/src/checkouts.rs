//! The checkouts open in the app, each scanned and watched once however many panels show it:
//! the changes panel subscribes to a checkout's project, and the file tree will join it there
//! rather than watch the disk again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use colosso_project::{GitDirs, Options, Project};
use gpui_kit::{App, Global};

/// The projects open, by checkout root. Each closes once the last panel holding it lets go.
#[derive(Default)]
struct Checkouts(HashMap<PathBuf, Weak<Project>>);

impl Global for Checkouts {}

/// The project for the checkout at `root`, opened unless a panel has it open already.
pub fn project(root: &Path, git: GitDirs, cx: &mut App) -> Arc<Project> {
    // By its real path, so a checkout reached through a link is the same checkout.
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let open = &mut cx.default_global::<Checkouts>().0;
    open.retain(|_, project| project.strong_count() > 0);
    if let Some(project) = open.get(&root).and_then(Weak::upgrade) {
        return project;
    }
    let project = Project::open(Options::new(root.clone(), Some(git)));
    open.insert(root, Arc::downgrade(&project));
    project
}
