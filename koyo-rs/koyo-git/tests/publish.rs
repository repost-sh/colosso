//! Committing a checkout's work and pushing it, against a remote of its own.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails loudly at the step that broke"
)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{git, scratch};
use koyo_git::{Checkout, Ref};

/// A checkout on `main` with a commit, and a bare remote it isn't pushed to yet.
fn checkout_and_remote() -> (PathBuf, PathBuf) {
    let root = scratch("publish");
    let remote = root.join("remote.git");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    git(&root, &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()]);
    git(&work, &["init", "--initial-branch=main"]);
    for setting in [
        ("user.name", "Test"),
        ("user.email", "test@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git(&work, &["config", setting.0, setting.1]);
    }
    fs::write(work.join("a.txt"), "one\n").unwrap();
    git(&work, &["add", "--all"]);
    git(&work, &["commit", "--message", "First"]);
    git(&work, &["remote", "add", "origin", remote.to_str().unwrap()]);
    (work, remote)
}

fn subject(dir: &Path) -> String {
    git(dir, &["log", "-1", "--format=%s"]).trim().to_owned()
}

#[test]
fn a_commit_takes_everything_changed_and_wants_a_message() {
    let (work, _) = checkout_and_remote();
    let checkout = Checkout::open(&work).unwrap();
    fs::write(work.join("new.txt"), "new\n").unwrap();
    fs::write(work.join("a.txt"), "two\n").unwrap();
    assert!(checkout.commit_all("  ").is_err());
    assert_eq!(subject(&work), "First");
    checkout.commit_all("Second").unwrap();
    assert_eq!(subject(&work), "Second");
    assert!(git(&work, &["status", "--porcelain"]).is_empty());
}

#[test]
fn a_branch_is_published_then_pushed_and_says_how_it_stands() {
    let (work, remote) = checkout_and_remote();
    let checkout = Checkout::open(&work).unwrap();
    let first = checkout.published();
    assert_eq!(first.remote.as_deref(), Some("origin"));
    assert_eq!(first.upstream, None);

    checkout.push().unwrap();
    let pushed = checkout.published();
    let upstream = Ref::Remote { remote: "origin".to_owned(), branch: "main".to_owned() };
    assert_eq!(pushed.upstream, Some(upstream));
    assert_eq!((pushed.ahead, pushed.behind), (0, 0));
    assert!(pushed.on_default);

    fs::write(work.join("a.txt"), "two\n").unwrap();
    checkout.commit_all("Second").unwrap();
    assert_eq!(checkout.published().ahead, 1);
    checkout.push().unwrap();
    assert_eq!(checkout.published().ahead, 0);
    assert_eq!(subject(&remote), "Second");
}

#[test]
fn a_branch_of_its_own_isnt_the_default_and_pulls_what_it_lacks() {
    let (work, _) = checkout_and_remote();
    let checkout = Checkout::open(&work).unwrap();
    checkout.push().unwrap();
    fs::write(work.join("a.txt"), "two\n").unwrap();
    checkout.commit_all("Second").unwrap();
    checkout.push().unwrap();
    git(&work, &["reset", "--hard", "HEAD~1"]);
    assert_eq!(checkout.published().behind, 1);
    checkout.pull().unwrap();
    assert_eq!(subject(&work), "Second");
    git(&work, &["switch", "--create", "feature"]);
    assert!(!checkout.published().on_default);
}

#[test]
fn a_fetch_says_what_was_pushed_from_elsewhere_and_leaves_the_branch_alone() {
    let (work, remote) = checkout_and_remote();
    let checkout = Checkout::open(&work).unwrap();
    checkout.fetch().unwrap();
    checkout.push().unwrap();
    let elsewhere = work.parent().unwrap().join("elsewhere");
    git(work.parent().unwrap(), &["clone", remote.to_str().unwrap(), elsewhere.to_str().unwrap()]);
    git(&elsewhere, &["commit", "--allow-empty", "--message", "From elsewhere"]);
    git(&elsewhere, &["push"]);
    assert_eq!(checkout.published().behind, 0, "it doesn't know yet");
    checkout.fetch().unwrap();
    assert_eq!(checkout.published().behind, 1);
    assert_eq!(subject(&work), "First");
}

#[test]
fn a_repository_with_no_remote_has_nowhere_to_push() {
    let (work, _) = checkout_and_remote();
    git(&work, &["remote", "remove", "origin"]);
    let checkout = Checkout::open(&work).unwrap();
    assert_eq!(checkout.published().remote, None);
    assert!(checkout.push().is_err());
}
