//! The checkouts open in the app, each scanned and watched once however many panels show it:
//! the changes panel subscribes to a checkout's project, and the file tree will join it there
//! rather than watch the disk again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use gpui_kit::{App, Global};
use koyo_git::Checkout as Git;
use koyo_project::{GitDirs, Options, Project};

use crate::blocking::Blocking;

/// The projects open, by checkout root. Each closes once the last panel holding it lets go.
#[derive(Default)]
struct Checkouts(HashMap<PathBuf, Weak<Project>>);

impl Global for Checkouts {}

/// A checkout's root as the file system names it, its links followed, so two paths to one
/// checkout open one project. Only [`open_git`] makes one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutRoot(PathBuf);

/// Git opened on a checkout, its folders as its project takes them, and its root.
pub struct Opened {
    pub git: Git,
    pub dirs: GitDirs,
    pub root: CheckoutRoot,
}

/// The project for the checkout at `root`, opened unless a panel has it open already.
pub fn project(root: &CheckoutRoot, git: GitDirs, cx: &mut App) -> Arc<Project> {
    let open = &mut cx.default_global::<Checkouts>().0;
    open.retain(|_, project| project.strong_count() > 0);
    if let Some(project) = open.get(&root.0).and_then(Weak::upgrade) {
        return project;
    }
    let project = Project::open(Options::new(root.0.clone(), Some(git)));
    open.insert(root.0.clone(), Arc::downgrade(&project));
    project
}

/// Git opened on the checkout at `path`.
pub fn open_git(path: &Path, _: Blocking<'_>) -> koyo_git::Result<Opened> {
    let git = Git::open(path)?;
    let (git_dir, common_dir) = git.git_dirs();
    let dirs = GitDirs {
        git_dir,
        common_dir,
    };
    let root = CheckoutRoot(
        std::fs::canonicalize(git.root()).unwrap_or_else(|_| git.root().to_owned()),
    );
    Ok(Opened { git, dirs, root })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::TestAppContext;

    use super::{open_git, project};
    use crate::blocking::in_test;
    use crate::test_support::{repo, scratch};

    /// A checkout reached through a link and by its own path is one project, opened once: the
    /// root it's known by is the file system's, which only opening git off the main thread
    /// asks for.
    #[gpui_kit::test]
    fn a_checkout_reached_through_a_link_is_the_project_its_own_path_opens(
        cx: &mut TestAppContext,
    ) {
        let root = repo("checkouts-linked", &[("README.md", "hi\n")]);
        let link = scratch("checkouts-link").join("linked");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let direct = in_test(|may| open_git(&root, may)).unwrap();
        let linked = in_test(|may| open_git(&link, may)).unwrap();
        assert_eq!(direct.root, linked.root);
        let first = cx.update(|cx| project(&direct.root, direct.dirs, cx));
        let second = cx.update(|cx| project(&linked.root, linked.dirs, cx));
        assert!(Arc::ptr_eq(&first, &second), "one project for both");
    }
}
