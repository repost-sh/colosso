//! The worktree lifecycle against real repositories: a bare "origin" and a clone of it, made
//! fresh for each test in the checkout's `target/test-repos`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use colosso_git::{Head, NewWorktree, Ref, Repo, Start};
use common::git;

struct Fixture {
    root: PathBuf,
    repo: Repo,
}

impl Fixture {
    /// An origin with one commit on `main` and a branch `shared`, cloned into `app`.
    fn new() -> Self {
        let root = common::scratch("worktrees");
        git(&root, &["init", "--bare", "-b", "main", "origin.git"]);
        git(&root, &["clone", "origin.git", "seed"]);
        let seed = root.join("seed");
        commit(&seed, "README.md", "hello");
        git(&seed, &["push", "origin", "main"]);
        git(&seed, &["switch", "-c", "shared"]);
        commit(&seed, "shared.txt", "shared");
        git(&seed, &["push", "origin", "shared"]);
        git(&root, &["clone", "origin.git", "app"]);
        let app = root.join("app");
        // The library's own git calls read the clone's config, so the user's hooks and
        // signing stay out of them too.
        git(&app, &["config", "core.hooksPath", "/dev/null"]);
        git(&app, &["config", "commit.gpgsign", "false"]);
        let repo = Repo::open(&app).unwrap();
        Fixture { root, repo }
    }

    fn workspaces(&self) -> PathBuf {
        self.root.join("workspaces").join(self.repo.name())
    }

    fn create(&self, name: &str, start: Start) -> colosso_git::Result<colosso_git::Worktree> {
        self.repo.create_worktree(&NewWorktree {
            name: name.into(),
            start,
            parent: self.workspaces(),
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", file]);
}

fn origin(branch: &str) -> Ref {
    Ref::Remote {
        remote: "origin".into(),
        branch: branch.into(),
    }
}

fn new_branch(prefix: &str) -> Start {
    Start::NewBranch {
        base: origin("main"),
        prefix: Some(prefix.into()),
    }
}

#[test]
fn a_clone_has_its_main_checkout_and_remote_branches() {
    let fixture = Fixture::new();
    let worktrees = fixture.repo.worktrees().unwrap();
    assert_eq!(worktrees.len(), 1);
    assert!(worktrees[0].is_main);
    assert_eq!(worktrees[0].head, Head::Branch("main".into()));

    let branches = fixture.repo.branches().unwrap();
    assert!(branches.contains(&Ref::Local("main".into())));
    assert!(branches.contains(&origin("shared")));
    assert!(!branches.iter().any(|r| r.branch() == "HEAD"));
    assert_eq!(fixture.repo.default_base(&branches), Some(origin("main")));
}

#[test]
fn opening_from_a_linked_worktree_finds_the_main_checkout() {
    let fixture = Fixture::new();
    let worktree = fixture.create("Payouts", new_branch("me")).unwrap();
    let reopened = Repo::open(&worktree.path).unwrap();
    assert_eq!(reopened.path(), fixture.repo.path());
}

#[test]
fn opening_a_folder_outside_any_repository_is_refused() {
    // The fixtures sit inside Colosso's own checkout, so ask about the filesystem root.
    let error = Repo::open(Path::new("/")).unwrap_err();
    assert_eq!(error.message, "/ is not in a git repository.");
}

#[test]
fn a_bare_repository_is_named_without_its_extension() {
    let fixture = Fixture::new();
    let bare = Repo::open(&fixture.root.join("origin.git")).unwrap();
    assert_eq!(bare.name(), "origin");
    assert!(bare.worktrees().unwrap().is_empty());
}

#[test]
fn a_new_branch_is_named_after_the_worktree_and_records_its_base() {
    let fixture = Fixture::new();
    let worktree = fixture.create("Fix Payouts", new_branch("me")).unwrap();
    assert_eq!(worktree.path, fixture.workspaces().join("fix-payouts"));
    assert_eq!(worktree.head, Head::Branch("me/fix-payouts".into()));
    assert!(worktree.path.join("README.md").exists());
    assert_eq!(
        fixture.repo.recorded_base("me/fix-payouts").as_deref(),
        Some("origin/main")
    );
    // Not tracking main, so a first push publishes a branch of its own.
    let upstream = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(&worktree.path)
        .output()
        .unwrap();
    assert!(!upstream.status.success());

    let listed = fixture.repo.worktrees().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[1].head, worktree.head);
}

#[test]
fn a_taken_name_gets_a_number() {
    let fixture = Fixture::new();
    fixture.create("payouts", new_branch("me")).unwrap();
    let second = fixture.create("payouts", new_branch("me")).unwrap();
    assert_eq!(second.head, Head::Branch("me/payouts-2".into()));
    assert_eq!(second.path, fixture.workspaces().join("payouts-2"));
}

#[test]
fn a_name_already_on_the_remote_is_not_reused() {
    let fixture = Fixture::new();
    let worktree = fixture
        .create(
            "shared",
            Start::NewBranch {
                base: origin("main"),
                prefix: None,
            },
        )
        .unwrap();
    assert_eq!(worktree.head, Head::Branch("shared-2".into()));
}

#[test]
fn a_new_branch_starts_from_the_latest_remote_commit() {
    let fixture = Fixture::new();
    let seed = fixture.root.join("seed");
    git(&seed, &["switch", "main"]);
    commit(&seed, "later.txt", "pushed after the clone");
    git(&seed, &["push", "origin", "main"]);

    let worktree = fixture.create("fresh", new_branch("me")).unwrap();
    assert!(worktree.path.join("later.txt").exists());
}

#[test]
fn an_unnamed_worktree_is_refused() {
    let fixture = Fixture::new();
    let error = fixture.create(" !! ", new_branch("me")).unwrap_err();
    assert_eq!(error.message, "Give the worktree a name.");
}

#[test]
fn an_existing_remote_branch_is_checked_out_tracking_it() {
    let fixture = Fixture::new();
    let worktree = fixture
        .create("", Start::Existing(origin("shared")))
        .unwrap();
    assert_eq!(worktree.head, Head::Branch("shared".into()));
    assert_eq!(worktree.path, fixture.workspaces().join("shared"));
    assert!(worktree.path.join("shared.txt").exists());
    let upstream = git(
        &worktree.path,
        &["rev-parse", "--abbrev-ref", "@{upstream}"],
    );
    assert_eq!(upstream.trim(), "origin/shared");
    // Colosso did not make this branch, so nothing marks it as one to delete.
    assert_eq!(fixture.repo.recorded_base("shared"), None);
}

#[test]
fn a_branch_checked_out_elsewhere_is_refused_by_git() {
    let fixture = Fixture::new();
    let error = fixture
        .create("again", Start::Existing(Ref::Local("main".into())))
        .unwrap_err();
    assert!(error.message.contains("already"), "{error}");
}

#[test]
fn removing_a_worktree_takes_its_changes_and_then_its_branch() {
    let fixture = Fixture::new();
    let worktree = fixture.create("scratch", new_branch("me")).unwrap();
    std::fs::write(worktree.path.join("notes.md"), "draft").unwrap();
    let loss = fixture.repo.inspect_removal(&worktree.path).unwrap();
    assert_eq!(loss.head, Head::Branch("me/scratch".into()));
    assert_eq!(loss.changes, ["notes.md"]);
    assert_eq!((loss.unmerged, loss.made_here), (0, true));

    fixture
        .repo
        .remove_worktree(&worktree.path, &loss, true)
        .unwrap();
    assert!(!worktree.path.exists());
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 1);
    assert!(
        !fixture
            .repo
            .branches()
            .unwrap()
            .contains(&Ref::Local("me/scratch".into()))
    );
}

#[test]
fn removing_refuses_once_more_would_be_lost_than_the_user_was_shown() {
    let fixture = Fixture::new();
    let worktree = fixture.create("busy", new_branch("me")).unwrap();
    let shown = fixture.repo.inspect_removal(&worktree.path).unwrap();
    std::fs::write(worktree.path.join("late.txt"), "written after asking").unwrap();
    assert!(
        fixture
            .repo
            .remove_worktree(&worktree.path, &shown, true)
            .is_err()
    );
    commit(&worktree.path, "late.txt", "committed after asking");
    assert!(
        fixture
            .repo
            .remove_worktree(&worktree.path, &shown, true)
            .is_err()
    );
    assert!(worktree.path.join("late.txt").exists());
}

#[test]
fn a_branch_kept_once_can_still_be_deleted_after_its_worktree_is_gone() {
    let fixture = Fixture::new();
    let worktree = fixture.create("twice", new_branch("me")).unwrap();
    let loss = fixture.repo.inspect_removal(&worktree.path).unwrap();
    fixture
        .repo
        .remove_worktree(&worktree.path, &loss, false)
        .unwrap();
    fixture
        .repo
        .remove_worktree(&worktree.path, &loss, true)
        .unwrap();
    assert!(
        !fixture
            .repo
            .branches()
            .unwrap()
            .contains(&Ref::Local("me/twice".into()))
    );
}

#[test]
fn commits_on_a_detached_worktree_count_as_lost() {
    let fixture = Fixture::new();
    let worktree = fixture.create("loose", new_branch("me")).unwrap();
    git(&worktree.path, &["switch", "--detach"]);
    commit(&worktree.path, "loose.txt", "on no branch");
    let loss = fixture.repo.inspect_removal(&worktree.path).unwrap();
    assert!(matches!(loss.head, Head::Detached(_)));
    assert_eq!(loss.unmerged, 1);
}

#[test]
fn ignored_files_are_counted_apart_from_changes() {
    let fixture = Fixture::new();
    let worktree = fixture.create("secrets", new_branch("me")).unwrap();
    std::fs::write(worktree.path.join(".gitignore"), ".env\n.gitignore\n").unwrap();
    std::fs::write(worktree.path.join(".env"), "TOKEN=1").unwrap();
    let loss = fixture.repo.inspect_removal(&worktree.path).unwrap();
    assert!(loss.changes.is_empty(), "{:?}", loss.changes);
    assert!(
        loss.ignored.contains(&".env".to_owned()),
        "{:?}",
        loss.ignored
    );
}

#[test]
fn commits_only_a_branch_has_count_as_unmerged_until_pushed() {
    let fixture = Fixture::new();
    let worktree = fixture.create("work", new_branch("me")).unwrap();
    assert_eq!(fixture.repo.unmerged_commits("me/work").unwrap(), 0);
    commit(&worktree.path, "one.txt", "1");
    commit(&worktree.path, "two.txt", "2");
    assert_eq!(fixture.repo.unmerged_commits("me/work").unwrap(), 2);
    git(&worktree.path, &["push", "origin", "me/work"]);
    assert_eq!(fixture.repo.unmerged_commits("me/work").unwrap(), 0);
}

#[test]
fn a_folder_deleted_behind_gits_back_drops_out_of_the_list() {
    let fixture = Fixture::new();
    let worktree = fixture.create("gone", new_branch("me")).unwrap();
    std::fs::remove_dir_all(&worktree.path).unwrap();
    assert_eq!(fixture.repo.worktrees().unwrap().len(), 1);
}
