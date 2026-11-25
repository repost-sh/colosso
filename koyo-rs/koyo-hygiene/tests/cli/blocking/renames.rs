use std::fs;

use super::support::{DEBT, with_debt};
use crate::imports::support::{HOST, HOST_DEBT, LIST, SOURCE};
use crate::support::Scratch;

/// Two blocking calls, among lines enough that a file holding them stays the same file through
/// an edit.
const READS: &str = "fn f() {\n    std::fs::read(\"x\");\n    std::fs::read(\"y\");\n}\n\nfn first() -> u32 {\n    1\n}\n\nfn second() -> u32 {\n    first() + 1\n}\n\nfn third() -> u32 {\n    second() + 1\n}\n";

/// A commit whose `a.rs` reads twice, as its list allows, and whose `b.rs` makes no call.
fn committed(name: &str) -> Scratch {
    let scratch = with_debt(
        &format!("renames-{name}"),
        &[
            ("main.rs", "mod a;\nmod b;\n"),
            ("a.rs", READS),
            ("b.rs", ""),
        ],
        "koyo/src/a.rs 2\n",
    );
    scratch.commit();
    scratch
}

fn source(file: &str) -> String {
    format!("{SOURCE}{file}")
}

/// Moves `a.rs` to `to`, a module `c` of the crate root, which the list names in its place.
fn moved_to(scratch: &Scratch, to: &str) {
    scratch.rename(&source("a.rs"), &source(to));
    scratch.write(
        &source("main.rs"),
        &format!("#[path = \"{to}\"]\nmod c;\nmod b;\n"),
    );
    scratch.write(DEBT, &format!("koyo/src/{to} 2\n"));
}

#[test]
fn a_renamed_file_keeps_its_allowance_whether_git_was_told_or_not() {
    let untracked = committed("untracked");
    moved_to(&untracked, "deep/c.rs");
    let run = untracked.run(&[]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", "")
    );

    let staged = committed("staged");
    moved_to(&staged, "deep/c.rs");
    staged.git(&["add", "-A"]);
    let run = staged.run(&[]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
}

#[test]
fn a_file_edited_as_it_moved_keeps_its_allowance() {
    let edited = READS.replace("first() + 1", "first() + 2");
    for (name, staged) in [("edited", false), ("staged-edited", true)] {
        let scratch = committed(name);
        moved_to(&scratch, "c.rs");
        if staged {
            scratch.git(&["add", "-A"]);
        }
        scratch.write(&source("c.rs"), &edited);
        let run = scratch.run(&[]);
        assert_eq!((run.code, run.stdout.as_str()), (0, ""), "{name}");
    }
}

#[test]
fn a_renamed_file_is_found_by_any_name_a_line_can_hold() {
    let scratch = committed("names");
    scratch.write(&source("with space.txt"), READS);
    scratch.commit();
    scratch.rename(&source("with space.txt"), &source("tab\tand \"quote\".txt"));
    moved_to(&scratch, "größe-名.rs");
    let run = scratch.run(&[]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", "")
    );
}

#[test]
fn a_raised_count_in_a_renamed_file_fails() {
    let scratch = committed("raised");
    moved_to(&scratch, "c.rs");
    scratch.write(
        &source("c.rs"),
        &READS.replacen('{', "{\n    std::fs::read(\"z\");", 1),
    );
    scratch.write(DEBT, "koyo/src/c.rs 3\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        ["koyo-rs/blocking-debt.txt:1:1: added call: koyo/src/c.rs 3: 2 on the last commit's list"]
    );
}

#[test]
fn a_copy_has_no_allowance() {
    let scratch = committed("copy");
    scratch.write(&source("c.rs"), READS);
    scratch.write(&source("main.rs"), "mod a;\nmod b;\nmod c;\n");
    scratch.write(DEBT, "koyo/src/a.rs 2\nkoyo/src/c.rs 2\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt.txt:2:1: added call: koyo/src/c.rs 2: not on the last commit's list"
        ]
    );
}

#[test]
fn two_files_do_not_both_inherit_one_allowance() {
    for (name, second) in [
        ("two", READS.to_owned()),
        ("two-edited", READS.replace("first() + 1", "first() + 2")),
    ] {
        let scratch = committed(name);
        moved_to(&scratch, "c.rs");
        scratch.write(&source("d.rs"), &second);
        scratch.write(&source("main.rs"), "mod b;\nmod c;\nmod d;\n");
        scratch.write(DEBT, "koyo/src/c.rs 2\nkoyo/src/d.rs 2\n");
        let run = scratch.run(&[]);
        assert_eq!(run.code, 1, "{name}");
        assert_eq!(
            run.findings(),
            [
                "koyo-rs/blocking-debt.txt:2:1: added call: koyo/src/d.rs 2: not on the last commit's list"
            ],
            "{name}"
        );
    }
}

#[test]
fn the_old_path_no_longer_counts() {
    let scratch = committed("old-path");
    moved_to(&scratch, "c.rs");
    scratch.write(DEBT, "koyo/src/a.rs 2\nkoyo/src/c.rs 2\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt.txt:1:1: gone call: koyo/src/a.rs holds 0, not 2",
            "koyo-rs/blocking-debt.txt:1:1: added call: koyo/src/a.rs 2: not on the last commit's list",
        ]
    );
    let pruned = scratch.run(&["--prune"]);
    assert_eq!(
        (pruned.code, pruned.stdout.as_str()),
        (
            0,
            "Pruned 0 edges from koyo-rs/module-edges.txt.\nLowered 1 line of koyo-rs/blocking-debt.txt.\n"
        )
    );
    assert_eq!(scratch.read(DEBT), "koyo/src/c.rs 2\n");
}

#[test]
fn a_file_unlike_the_one_that_went_has_no_allowance() {
    let scratch = committed("unlike");
    moved_to(&scratch, "c.rs");
    scratch.write(
        &source("c.rs"),
        "struct Other;\n\nimpl Other {\n    fn read(&self) {\n        std::fs::read(\"x\");\n        std::fs::read(\"y\");\n    }\n}\n",
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt.txt:1:1: added call: koyo/src/c.rs 2: not on the last commit's list"
        ]
    );
}

#[test]
fn a_file_moved_to_the_other_crate_leaves_its_allowance_behind() {
    let scratch = committed("other-crate");
    scratch.rename(&source("a.rs"), &format!("{HOST}a.rs"));
    scratch.write(&source("main.rs"), "mod b;\n");
    scratch.write(&format!("{HOST}lib.rs"), "mod a;\n");
    scratch.write(DEBT, "");
    scratch.write(HOST_DEBT, "koyo-browser-host/src/a.rs 2\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt-browser-host.txt:1:1: added call: koyo-browser-host/src/a.rs 2: not on the last commit's list"
        ]
    );
}

#[test]
fn before_the_commit_that_adds_the_list_a_renamed_file_is_compared_with_nothing() {
    let scratch = with_debt(
        "renames-first-commit",
        &[("main.rs", "mod a;\n"), ("a.rs", READS)],
        "",
    );
    for list in [LIST, DEBT, HOST_DEBT] {
        fs::remove_file(scratch.path(list)).expect("remove the list");
    }
    scratch.commit();
    moved_to(&scratch, "c.rs");
    scratch.write(&source("main.rs"), "mod c;\n");
    scratch.write(LIST, "");
    scratch.write(HOST_DEBT, "");
    let run = scratch.run(&[]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
}

#[test]
fn finding_a_rename_leaves_the_index_and_the_checkout_as_they_were() {
    let scratch = committed("untouched");
    moved_to(&scratch, "c.rs");
    let index = fs::read(scratch.path(".git/index")).expect("the index");
    let run = scratch.run(&[]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
    assert_eq!(
        fs::read(scratch.path(".git/index")).expect("the index"),
        index
    );
    let kept: Vec<_> = fs::read_dir(scratch.path("koyo-rs/target/tmp"))
        .expect("the tool's scratch folder")
        .collect();
    assert!(kept.is_empty(), "{kept:?}");
}
