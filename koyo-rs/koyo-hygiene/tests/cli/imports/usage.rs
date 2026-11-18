use super::support::{LIST, checkout};
use crate::support::Scratch;

/// Modules `a` and `b` import each other, and the list holds only `a -> b`; `c.rs` has a
/// comment.
fn both_rules_broken(name: &str) -> Scratch {
    let scratch = checkout(
        name,
        &[
            ("main.rs", "mod a;\nmod b;\nmod c;\n"),
            ("a.rs", "use crate::b::B;\n"),
            ("b.rs", "use crate::a::A;\n"),
            ("c.rs", "// note\nfn f() {}\n"),
        ],
    );
    scratch.write(LIST, "a b\n");
    scratch
}

#[test]
fn with_no_argument_both_checks_run_and_report_together() {
    let run = both_rules_broken("usage-both").run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/koyo/src/b.rs:1:12: new edge: b -> a closes b -> a -> b",
            "koyo-rs/koyo/src/c.rs:1:1: comment: // note",
        ]
    );
    assert!(
        run.stdout
            .contains("\n2 findings in 2 files (1 comment, 1 new edge).\n"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_path_or_fix_runs_the_comment_check_alone() {
    let scratch = both_rules_broken("usage-comments");
    let run = scratch.run(&["koyo-rs/koyo/src"]);
    assert_eq!(
        run.findings(),
        ["koyo-rs/koyo/src/c.rs:1:1: comment: // note"]
    );
    let fix = scratch.run(&["--fix"]);
    assert_eq!(
        (fix.code, fix.stdout.as_str()),
        (0, "Deleted 1 comment from 1 file.\n")
    );
}

#[test]
fn the_comments_are_still_reported_when_the_import_check_cannot_run() {
    let scratch = both_rules_broken("usage-import-failure");
    scratch.write(LIST, "a\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 2);
    assert_eq!(
        run.findings(),
        ["koyo-rs/koyo/src/c.rs:1:1: comment: // note"]
    );
    assert!(
        run.stderr.contains("koyo-rs/module-edges.txt:1"),
        "{}",
        run.stderr
    );
}

#[test]
fn prune_and_stats_go_alone() {
    let scratch = both_rules_broken("usage-alone");
    for arguments in [
        &["--prune", "--stats"][..],
        &["--prune", "koyo-rs/koyo/src"],
        &["--fix", "--prune"],
        &["--stats", "--fix"],
        &["--stats", "--stats"],
    ] {
        let run = scratch.run(arguments);
        assert_eq!(run.code, 2, "{arguments:?}");
        assert!(run.stdout.is_empty(), "{arguments:?}: {}", run.stdout);
        assert!(
            run.stderr.contains("go alone"),
            "{arguments:?}: {}",
            run.stderr
        );
    }
    assert_eq!(scratch.read(LIST), "a b\n");
    assert_eq!(
        scratch.read("koyo-rs/koyo/src/c.rs"),
        "// note\nfn f() {}\n"
    );
}
