//! What the app's tests share: folders of their own under the target folder, git run in them
//! as a user would, with no hooks or signing of theirs, and the shell in a window over them.
//! The visual checks share the git part, to make their fixtures.

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Point, Render, Size, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};

#[cfg(test)]
use crate::projects::read_worktrees;
#[cfg(test)]
use crate::shell::Shell;

/// A folder for one test under the target folder, empty.
#[cfg(test)]
pub fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-repos")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::canonicalize(root).unwrap()
}

/// Runs git with `args` in `root`, or says why it didn't succeed.
pub fn try_git(root: &Path, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("git {args:?}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&output.stderr);
        Err(format!("git {args:?}: {}", said.trim()))
    }
}

/// Runs git with `args` in `root`, which has to succeed.
#[cfg(test)]
pub fn git(root: &Path, args: &[&str]) {
    try_git(root, args).unwrap();
}

/// Writes `contents` to the file at `path` in `root`, making the folders it's in.
#[cfg(test)]
pub fn write(root: &Path, path: &str, contents: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A repository of its own, `files` in it committed, on `main`.
#[cfg(test)]
pub fn repo(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = scratch(name);
    for (path, text) in files {
        write(&root, path, text.as_bytes());
    }
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "first"]);
    root
}

/// What `build` makes, in a window of its own of `size`.
#[cfg(test)]
pub fn in_window<V: Render>(
    size: Size<gpui_kit::Pixels>,
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size,
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..WindowOptions::default()
        };
        gpui_kit::open_window(options, cx, build).unwrap()
    })
}

/// The shell in a laptop-sized window, each of `roots` a project in its sidebar.
#[cfg(test)]
pub fn shell_with(roots: &[&Path], cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    // Each checkout's project lists and watches it on a thread of its own.
    cx.executor().allow_parking();
    cx.update(crate::init);
    let (window, shell) = in_window(size(px(1400.), px(900.)), cx, |window, cx| {
        cx.new(|cx| Shell::new(window, cx))
    });
    shell.update(cx, |shell, cx| {
        for root in roots {
            shell.workspace.add_project(root.to_path_buf());
            shell.show_worktrees(root, read_worktrees(root), cx);
        }
    });
    (window, shell)
}

/// Selects the checkout at `root` in the sidebar.
#[cfg(test)]
pub fn select(shell: &Entity<Shell>, root: &Path, cx: &mut TestAppContext) {
    shell.update(cx, |shell, cx| {
        let project = shell.workspace.project_index(root).unwrap();
        assert!(shell.workspace.select_worktree(project, root));
        cx.notify();
    });
    cx.run_until_parked();
}
