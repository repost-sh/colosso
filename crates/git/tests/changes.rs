//! A checkout's changes against real repositories, made fresh for each test in the checkout's
//! `target/test-repos`.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly at the step that broke"
)]

mod common;

use std::path::PathBuf;

use colosso_git::{Change, Checkout, Content, Discarding, Hunk, Status};

/// A repository with `files` committed, removed again when the test ends.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let fixture = Fixture::unborn();
        for (path, text) in files {
            fixture.write(path, text);
        }
        if !files.is_empty() {
            fixture.git(&["add", "-A"]);
            fixture.git(&["commit", "-m", "start"]);
        }
        fixture
    }

    /// A repository without a commit.
    fn unborn() -> Self {
        let fixture = Fixture {
            root: common::scratch("changes"),
        };
        fixture.git(&["init", "-b", "main"]);
        fixture
    }

    fn write(&self, path: &str, text: impl AsRef<[u8]>) {
        let path = self.root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn remove(&self, path: &str) {
        std::fs::remove_file(self.root.join(path)).unwrap();
    }

    fn git(&self, args: &[&str]) {
        common::git(&self.root, args);
    }

    fn changes(&self) -> Vec<Change> {
        Checkout::open(&self.root).unwrap().changes().unwrap()
    }

    fn diff(&self, path: &str) -> Content {
        let checkout = Checkout::open(&self.root).unwrap();
        let change = checkout
            .changes()
            .unwrap()
            .into_iter()
            .find(|change| change.path == path)
            .unwrap_or_else(|| panic!("{path} isn't changed"));
        checkout.diffs(&[change]).remove(0).content
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn staged(path: &str, status: Status) -> Change {
    Change {
        path: path.into(),
        status,
        staged: true,
    }
}

fn unstaged(path: &str, status: Status) -> Change {
    Change {
        path: path.into(),
        status,
        staged: false,
    }
}

fn text(content: &Content) -> (Vec<&str>, Vec<&str>, &[Hunk]) {
    match content {
        Content::Text { old, new, hunks } => (old.iter().collect(), new.iter().collect(), hunks),
        other => panic!("not text: {other:?}"),
    }
}

#[test]
fn a_clean_checkout_has_no_changes() {
    let fixture = Fixture::new(&[("README.md", "hello\n")]);
    assert!(fixture.changes().is_empty());
}

#[test]
fn staged_changes_come_first_then_the_rest_by_path() {
    let fixture = Fixture::new(&[
        ("README.md", "hello\n"),
        ("src/lib.rs", "fn a() {}\n"),
        ("old.txt", "going\n"),
    ]);
    fixture.write("README.md", "hello\nworld\n");
    fixture.write("src/new.rs", "fn b() {}\n");
    fixture.write("staged.rs", "fn c() {}\n");
    fixture.git(&["add", "staged.rs"]);
    fixture.remove("old.txt");
    assert_eq!(
        fixture.changes(),
        [
            staged("staged.rs", Status::Added),
            unstaged("README.md", Status::Modified),
            unstaged("old.txt", Status::Deleted),
            unstaged("src/new.rs", Status::Untracked),
        ]
    );
}

#[test]
fn ignored_files_are_left_out() {
    let fixture = Fixture::new(&[(".gitignore", "target/\n.env\n"), ("a.txt", "a\n")]);
    fixture.write("target/debug/app", "binary");
    fixture.write(".env", "SECRET=1");
    assert!(fixture.changes().is_empty());
}

#[test]
fn a_change_staged_then_undone_on_disk_shows_on_both_sides() {
    let fixture = Fixture::new(&[("a.txt", "a\n")]);
    fixture.write("a.txt", "b\n");
    fixture.git(&["add", "a.txt"]);
    fixture.write("a.txt", "a\n");
    // Staged as new, then deleted from disk.
    fixture.write("gone.txt", "x\n");
    fixture.git(&["add", "gone.txt"]);
    fixture.remove("gone.txt");
    assert_eq!(
        fixture.changes(),
        [
            staged("a.txt", Status::Modified),
            staged("gone.txt", Status::Added),
            unstaged("a.txt", Status::Modified),
            unstaged("gone.txt", Status::Deleted),
        ]
    );
}

#[test]
fn a_partly_staged_file_has_a_diff_for_each_part() {
    let fixture = Fixture::new(&[("a.txt", "one\ntwo\nthree\n")]);
    fixture.write("a.txt", "ONE\ntwo\nthree\n");
    fixture.git(&["add", "a.txt"]);
    fixture.write("a.txt", "ONE\ntwo\nTHREE\n");
    let checkout = Checkout::open(&fixture.root).unwrap();
    let diffs = checkout.diffs(&checkout.changes().unwrap());
    let sides: Vec<_> = diffs
        .iter()
        .map(|diff| (diff.change.staged, text(&diff.content)))
        .collect();
    let index = vec!["ONE", "two", "three"];
    let (staged, (_, staged_new, staged_hunks)) = &sides[0];
    assert!(*staged);
    assert_eq!(*staged_new, index);
    assert_eq!(
        *staged_hunks,
        [Hunk {
            old: 0..1,
            new: 0..1
        }]
    );
    let (staged, (unstaged_old, unstaged_new, unstaged_hunks)) = &sides[1];
    assert!(!*staged);
    assert_eq!(*unstaged_old, index);
    assert_eq!(*unstaged_new, ["ONE", "two", "THREE"]);
    assert_eq!(
        *unstaged_hunks,
        [Hunk {
            old: 2..3,
            new: 2..3
        }]
    );
}

#[test]
fn a_mode_change_alone_is_still_a_change() {
    let fixture = Fixture::new(&[("run.sh", "echo hi\n")]);
    fixture.git(&["update-index", "--chmod=+x", "run.sh"]);
    fixture.git(&["commit", "-m", "executable"]);
    fixture.git(&["update-index", "--chmod=-x", "run.sh"]);
    let checkout = Checkout::open(&fixture.root).unwrap();
    let diffs = checkout.diffs(&checkout.changes().unwrap());
    assert_eq!(diffs.len(), 1, "{diffs:?}");
}

#[test]
fn a_moved_file_is_a_rename_staged_or_not() {
    let fixture = Fixture::new(&[
        ("src/old.rs", "fn one() {}\nfn two() {}\nfn three() {}\n"),
        ("staged/before.rs", "fn four() {}\nfn five() {}\n"),
    ]);
    std::fs::rename(
        fixture.root.join("src/old.rs"),
        fixture.root.join("src/new.rs"),
    )
    .unwrap();
    fixture.git(&["mv", "staged/before.rs", "staged/after.rs"]);
    assert_eq!(
        fixture.changes(),
        [
            staged(
                "staged/after.rs",
                Status::Renamed {
                    from: "staged/before.rs".into()
                }
            ),
            unstaged(
                "src/new.rs",
                Status::Renamed {
                    from: "src/old.rs".into()
                }
            ),
        ]
    );
    // Diffed against where it came from, so an unchanged move has no hunks.
    let moved = fixture.diff("src/new.rs");
    let (old, new, hunks) = text(&moved);
    assert_eq!(old, new);
    assert!(hunks.is_empty());
}

#[test]
fn a_diff_has_both_sides_and_where_they_differ() {
    let fixture = Fixture::new(&[("a.txt", "one\ntwo\nthree\nfour\n")]);
    fixture.write("a.txt", "one\n2\nthree\nfour\nfive\n");
    let content = fixture.diff("a.txt");
    let (old, new, hunks) = text(&content);
    assert_eq!(old, ["one", "two", "three", "four"]);
    assert_eq!(new, ["one", "2", "three", "four", "five"]);
    assert_eq!(
        hunks,
        [
            Hunk {
                old: 1..2,
                new: 1..2
            },
            Hunk {
                old: 4..4,
                new: 4..5
            },
        ]
    );
    assert_eq!(content.line_counts(), (2, 1));
}

#[test]
fn an_added_or_deleted_file_has_one_empty_side() {
    let fixture = Fixture::new(&[("gone.txt", "a\nb\n")]);
    fixture.remove("gone.txt");
    fixture.write("new.txt", "c\n");
    let deleted = fixture.diff("gone.txt");
    let (old, new, _) = text(&deleted);
    assert_eq!((old.len(), new.len()), (2, 0));
    assert_eq!(deleted.line_counts(), (0, 2));
    assert_eq!(fixture.diff("new.txt").line_counts(), (1, 0));
}

#[test]
fn line_endings_and_a_missing_last_newline_stay_out_of_the_lines() {
    let fixture = Fixture::new(&[("dos.txt", "a\r\nb\r\n")]);
    fixture.write("dos.txt", "a\r\nc");
    let content = fixture.diff("dos.txt");
    let (old, new, _) = text(&content);
    assert_eq!(old, ["a", "b"]);
    assert_eq!(new, ["a", "c"]);
}

#[test]
fn binary_and_huge_files_are_not_diffed() {
    let fixture = Fixture::new(&[("logo.png", "")]);
    fixture.write("logo.png", [0x89, b'P', b'N', b'G', 0, 0, 1, 2]);
    fixture.write("huge.txt", "line\n".repeat(2 << 20));
    assert!(matches!(fixture.diff("logo.png"), Content::Binary));
    assert!(matches!(
        fixture.diff("huge.txt"),
        Content::TooLarge { bytes } if bytes == 10 << 20
    ));
}

#[test]
fn before_the_first_commit_everything_is_untracked() {
    let fixture = Fixture::unborn();
    fixture.write("a.txt", "a\n");
    assert_eq!(fixture.changes(), [unstaged("a.txt", Status::Untracked)]);
    assert_eq!(fixture.diff("a.txt").line_counts(), (1, 0));
}

#[test]
fn many_files_are_diffed_in_their_order() {
    let files: Vec<(String, String)> = (0..64)
        .map(|n| (format!("f{n:02}.txt"), format!("{n}\n")))
        .collect();
    let committed: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let fixture = Fixture::new(&committed);
    for (path, _) in &files {
        fixture.write(path, "changed\n");
    }
    let checkout = Checkout::open(&fixture.root).unwrap();
    let changes = checkout.changes().unwrap();
    let diffs = checkout.diffs(&changes);
    let order: Vec<&Change> = diffs.iter().map(|diff| &diff.change).collect();
    assert_eq!(order, changes.iter().collect::<Vec<_>>());
    assert!(
        diffs
            .iter()
            .all(|diff| diff.content.line_counts() == (1, 1))
    );
}

#[test]
fn staging_and_unstaging_moves_changes_between_the_sides() {
    let fixture = Fixture::new(&[("a.txt", "a\n"), ("gone.txt", "x\n")]);
    fixture.write("a.txt", "b\n");
    fixture.write("new.txt", "n\n");
    fixture.remove("gone.txt");
    let checkout = Checkout::open(&fixture.root).unwrap();
    checkout.stage(&checkout.changes().unwrap()).unwrap();
    assert_eq!(
        fixture.changes(),
        [
            staged("a.txt", Status::Modified),
            staged("gone.txt", Status::Deleted),
            staged("new.txt", Status::Added),
        ]
    );
    checkout.unstage(&checkout.changes().unwrap()).unwrap();
    assert_eq!(
        fixture.changes(),
        [
            unstaged("a.txt", Status::Modified),
            unstaged("gone.txt", Status::Deleted),
            unstaged("new.txt", Status::Untracked),
        ]
    );
}

#[test]
fn unstaging_works_before_the_first_commit() {
    let fixture = Fixture::unborn();
    fixture.write("a.txt", "a\n");
    let checkout = Checkout::open(&fixture.root).unwrap();
    checkout.stage(&checkout.changes().unwrap()).unwrap();
    assert_eq!(fixture.changes(), [staged("a.txt", Status::Added)]);
    checkout.unstage(&checkout.changes().unwrap()).unwrap();
    assert_eq!(fixture.changes(), [unstaged("a.txt", Status::Untracked)]);
}

// Discarding moves new files to the trash, outside the checkout, so these discard only what
// git puts back.

#[test]
fn discarding_what_is_unstaged_keeps_what_is_staged() {
    let fixture = Fixture::new(&[("a.txt", "one\n"), ("gone.txt", "x\n")]);
    fixture.write("a.txt", "staged\n");
    fixture.git(&["add", "a.txt"]);
    fixture.write("a.txt", "on disk\n");
    fixture.remove("gone.txt");
    let checkout = Checkout::open(&fixture.root).unwrap();
    let unstaged: Vec<Change> = (checkout.changes().unwrap().into_iter())
        .filter(|change| !change.staged)
        .collect();
    checkout.discard(&unstaged).unwrap();
    assert_eq!(fixture.changes(), [staged("a.txt", Status::Modified)]);
    let read = |path: &str| std::fs::read_to_string(fixture.root.join(path)).unwrap();
    assert_eq!(read("a.txt"), "staged\n");
    assert_eq!(read("gone.txt"), "x\n");
}

#[test]
fn discarding_a_staged_change_goes_back_to_the_commit() {
    let fixture = Fixture::new(&[("a.txt", "one\n")]);
    fixture.write("a.txt", "staged\n");
    fixture.git(&["add", "a.txt"]);
    fixture.write("a.txt", "on disk\n");
    let checkout = Checkout::open(&fixture.root).unwrap();
    checkout
        .discard(&[staged("a.txt", Status::Modified)])
        .unwrap();
    assert!(fixture.changes().is_empty());
}

#[test]
fn only_new_files_go_to_the_trash() {
    let discarding = |change: Change| change.discarding();
    assert_eq!(
        discarding(unstaged("a", Status::Untracked)),
        Discarding::Trashed
    );
    assert_eq!(discarding(staged("a", Status::Added)), Discarding::Trashed);
    // Modified on disk, but staged as new: what's staged is still there to go back to.
    assert_eq!(
        discarding(unstaged("a", Status::Modified)),
        Discarding::Reverted
    );
    assert_eq!(
        discarding(staged("a", Status::Deleted)),
        Discarding::Restored
    );
    assert_eq!(
        discarding(unstaged("a", Status::Conflicted)),
        Discarding::Kept
    );
}
