use std::fs;

use super::support::{DEBT, calls, with_debt};
use crate::support::Scratch;

const PRUNE: &str = "script/cargo run -q -p koyo-hygiene -- --prune";
const SPAWN: &str = "blocking::spawn(cx.background_executor(), |may| ...)";

/// `a.rs` reads twice and `b.rs` once, and the list allows each what it holds.
fn reads(name: &str) -> Scratch {
    with_debt(
        name,
        &[
            ("main.rs", "mod a;\nmod b;\nmod c;\n"),
            (
                "a.rs",
                "fn f() {\n    std::fs::read(\"x\");\n    std::fs::read(\"y\");\n}\n",
            ),
            ("b.rs", "fn f() {\n    std::fs::write(\"x\", \"\");\n}\n"),
            ("c.rs", ""),
        ],
        "# kept as written\nkoyo/src/a.rs 2\n\nkoyo/src/b.rs 1\n",
    )
}

#[test]
fn files_holding_what_the_list_allows_exit_0_and_say_nothing() {
    let run = reads("clean").run(&[]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", "")
    );
}

#[test]
fn a_call_in_a_file_the_list_lacks_fails_with_its_place_and_the_way_off_the_main_thread() {
    let scratch = reads("unlisted");
    scratch.write(
        "koyo-rs/koyo/src/c.rs",
        "fn f() {\n    std::fs::read(\"x\");\n}\n",
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(calls(&run), ["c.rs:2:5: std::fs::read"]);
    assert!(run.stdout.contains(SPAWN), "{}", run.stdout);
    assert!(!run.stdout.contains(PRUNE), "{}", run.stdout);
}

#[test]
fn a_listed_file_that_gains_a_call_fails_at_each_of_its_calls() {
    let scratch = reads("grew");
    scratch.write(
        "koyo-rs/koyo/src/b.rs",
        "fn f() {\n    std::fs::write(\"x\", \"\");\n    std::path::Path::new(\"x\").exists();\n}\n",
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        calls(&run),
        [
            "b.rs:2:5: std::fs::write (2 in the file, koyo-rs/blocking-debt.txt allows 1)",
            "b.rs:3:31: .exists() (2 in the file, koyo-rs/blocking-debt.txt allows 1)",
        ]
    );
}

#[test]
fn a_file_holding_fewer_than_its_line_is_a_gone_call_that_names_prune() {
    let scratch = reads("gone");
    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "fn f() {\n    std::fs::read(\"x\");\n}\n",
    );
    scratch.write("koyo-rs/koyo/src/b.rs", "");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt.txt:2:1: gone call: koyo/src/a.rs holds 1, not 2",
            "koyo-rs/blocking-debt.txt:4:1: gone call: koyo/src/b.rs holds 0, not 1",
        ]
    );
    assert!(run.stdout.contains(PRUNE), "{}", run.stdout);
}

#[test]
fn prune_lowers_each_line_to_what_its_file_holds_and_never_raises_one() {
    let scratch = reads("prune");
    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "fn f() {\n    std::fs::read(\"x\");\n}\n",
    );
    scratch.write(
        "koyo-rs/koyo/src/b.rs",
        "fn f() {\n    std::fs::read(\"x\");\n    std::fs::read(\"y\");\n}\n",
    );
    scratch.write("koyo-rs/koyo/src/c.rs", "");
    scratch.write(
        DEBT,
        "# kept as written\nkoyo/src/a.rs 2\n\nkoyo/src/b.rs 1\nkoyo/src/c.rs 4\n",
    );
    let run = scratch.run(&["--prune"]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.stdout.lines().take(2).collect::<Vec<_>>(),
        [
            "Pruned 0 edges from koyo-rs/module-edges.txt.",
            "Lowered 2 lines of koyo-rs/blocking-debt.txt.",
        ]
    );
    assert_eq!(
        scratch.read(DEBT),
        "# kept as written\nkoyo/src/a.rs 1\n\nkoyo/src/b.rs 1\n"
    );
    assert_eq!(calls(&run).len(), 2, "{}", run.stdout);
    let again = scratch.run(&["--prune"]);
    assert_eq!(
        again.stdout.lines().nth(1),
        Some(
            "koyo-rs/koyo/src/b.rs:2:5: blocking call: std::fs::read (2 in the file, koyo-rs/blocking-debt.txt allows 1)"
        )
    );
}

#[test]
fn a_line_the_last_commits_list_lacks_or_allows_less_fails_the_list_only_shrinks() {
    let scratch = reads("added");
    scratch.commit();
    scratch.write(
        "koyo-rs/koyo/src/b.rs",
        "fn f() {\n    std::fs::read(\"x\");\n    std::fs::read(\"y\");\n}\n",
    );
    scratch.write(
        "koyo-rs/koyo/src/c.rs",
        "fn f() {\n    std::fs::read(\"x\");\n}\n",
    );
    scratch.write(DEBT, "koyo/src/a.rs 2\nkoyo/src/b.rs 2\nkoyo/src/c.rs 1\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/blocking-debt.txt:2:1: added call: koyo/src/b.rs 2: 1 on the last commit's list",
            "koyo-rs/blocking-debt.txt:3:1: added call: koyo/src/c.rs 1: not on the last commit's list",
        ]
    );
    assert!(run.stdout.contains("only shrinks"), "{}", run.stdout);

    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "fn f() {\n    std::fs::read(\"x\");\n}\n",
    );
    scratch.write("koyo-rs/koyo/src/b.rs", "");
    scratch.write("koyo-rs/koyo/src/c.rs", "");
    scratch.write(DEBT, "koyo/src/a.rs 1\n");
    let lowered = scratch.run(&[]);
    assert_eq!((lowered.code, lowered.stdout.as_str()), (0, ""));
}

#[test]
fn the_tool_fails_with_2_when_the_list_cannot_be_read() {
    let missing = reads("missing-debt");
    fs::remove_file(missing.path(DEBT)).expect("remove the list");
    let run = missing.run(&[]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr
            .contains("git checkout HEAD -- koyo-rs/blocking-debt.txt"),
        "{}",
        run.stderr
    );

    for (name, list, says) in [
        (
            "malformed-debt",
            "koyo/src/a.rs 2 3\n",
            "koyo-rs/blocking-debt.txt:1",
        ),
        (
            "uncounted-debt",
            "koyo/src/a.rs many\n",
            "\"many\" isn't a count",
        ),
        (
            "twice-debt",
            "koyo/src/a.rs 2\nkoyo/src/a.rs 1\n",
            "listed twice",
        ),
    ] {
        let scratch = reads(name);
        scratch.write(DEBT, list);
        let run = scratch.run(&[]);
        assert_eq!(run.code, 2, "{name}");
        assert!(run.stderr.contains(says), "{name}: {}", run.stderr);
    }
}
