//! What the tests share: a folder of their own in the checkout, and git run for their setup.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A new empty folder under the checkout's `target/test-repos`, named after `kind` and a number
/// of its own, spelled the way git reports paths.
pub fn scratch(kind: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    // Not CARGO_TARGET_TMPDIR: a shared build dir can put that outside the checkout.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-repos")
        .join(format!(
            "{kind}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    // Without the `..` above.
    std::fs::canonicalize(&root).unwrap()
}

/// Runs git in `dir` for a test's own setup, without the user's signing or hooks getting in,
/// and returns what it printed.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
