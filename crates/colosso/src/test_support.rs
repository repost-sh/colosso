//! What the app's tests share: folders of their own under the target folder, and git run in
//! them as a user would, with no hooks or signing of theirs.

use std::path::{Path, PathBuf};

/// A folder for one test under the target folder, empty.
pub fn scratch(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-repos")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::canonicalize(root).unwrap()
}

/// Runs git with `args` in `root`, which has to succeed.
pub fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
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
        .unwrap()
        .status;
    assert!(status.success(), "git {args:?}");
}

/// Writes `contents` to the file at `path` in `root`, making the folders it's in.
pub fn write(root: &Path, path: &str, contents: &[u8]) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}
