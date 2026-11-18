use std::fs;

use super::support::{LIST, checkout};
use crate::support::Scratch;

const NEW_FIX: &str = "an argument or an event instead";
const ONLY_SHRINKS: &str = "only shrinks";
const PRUNE: &str = "script/cargo run -q -p koyo-hygiene -- --prune";

/// Modules `a` and `b` import each other, and `c` and `d` import nothing.
fn cycle_of_two(name: &str) -> Scratch {
    let scratch = checkout(
        name,
        &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            ("a.rs", "use crate::b::B;\n"),
            ("b.rs", "use crate::a::A;\n"),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
    );
    scratch.write(LIST, "a b\nb a\n");
    scratch
}

#[test]
fn a_tree_whose_cycles_are_all_listed_exits_0_and_says_nothing() {
    let run = cycle_of_two("clean").run(&[]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", "")
    );
}

#[test]
fn an_edge_to_a_module_that_cannot_reach_back_is_free() {
    let scratch = cycle_of_two("free");
    scratch.write("koyo-rs/koyo/src/c.rs", "use crate::a::A;\n");
    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "use crate::b::B;\nuse crate::d::D;\n",
    );
    let run = scratch.run(&[]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
}

#[test]
fn an_edge_that_closes_a_cycle_fails_with_its_place_and_the_cycle() {
    let scratch = cycle_of_two("closes");
    scratch.write("koyo-rs/koyo/src/c.rs", "use crate::a::A;\n");
    scratch.write(
        "koyo-rs/koyo/src/b.rs",
        "use crate::a::A;\nfn f() {\n    crate::c::g();\n}\n",
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/koyo/src/b.rs:3:12: new edge: b -> c closes b -> c -> a -> b",
            "koyo-rs/koyo/src/c.rs:1:12: new edge: c -> a closes c -> a -> b -> c",
        ]
    );
    assert!(run.stdout.contains(NEW_FIX), "{}", run.stdout);
    assert!(!run.stdout.contains(PRUNE), "{}", run.stdout);
}

#[test]
fn an_edge_that_leaves_every_cycle_is_gone_and_names_prune() {
    let scratch = cycle_of_two("gone");
    scratch.write("koyo-rs/koyo/src/b.rs", "");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/module-edges.txt:1:1: gone edge: a -> b",
            "koyo-rs/module-edges.txt:2:1: gone edge: b -> a",
        ]
    );
    assert!(run.stdout.contains(PRUNE), "{}", run.stdout);
    assert!(!run.stdout.contains(NEW_FIX), "{}", run.stdout);
}

#[test]
fn prune_takes_off_only_the_edges_off_every_cycle_and_never_adds() {
    let scratch = cycle_of_two("prune");
    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "use crate::b::B;\nuse crate::d::D;\n",
    );
    scratch.write("koyo-rs/koyo/src/c.rs", "use crate::b::B;\n");
    scratch.write("koyo-rs/koyo/src/d.rs", "use crate::a::A;\n");
    scratch.write(LIST, "# kept as written\nb a\nc b\na b\n\nb c\n");
    let expected = [
        "koyo-rs/koyo/src/a.rs:2:12: new edge: a -> d closes a -> d -> a",
        "koyo-rs/koyo/src/d.rs:1:12: new edge: d -> a closes d -> a -> d",
    ];
    let run = scratch.run(&["--prune"]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.stdout.lines().next(),
        Some("Pruned 2 edges from koyo-rs/module-edges.txt.")
    );
    assert_eq!(run.findings()[1..], expected);
    assert_eq!(scratch.read(LIST), "# kept as written\nb a\na b\n\n");
    let again = scratch.run(&["--prune"]);
    assert_eq!(
        again.stdout.lines().next(),
        Some("Pruned 0 edges from koyo-rs/module-edges.txt.")
    );
    assert_eq!(again.findings()[1..], expected);
    assert_eq!(scratch.read(LIST), "# kept as written\nb a\na b\n\n");
}

#[test]
fn prune_with_nothing_gone_exits_0() {
    let scratch = cycle_of_two("prune-clean");
    let run = scratch.run(&["--prune"]);
    assert_eq!(
        (run.code, run.stdout.as_str()),
        (0, "Pruned 0 edges from koyo-rs/module-edges.txt.\n")
    );
    assert_eq!(scratch.read(LIST), "a b\nb a\n");
}

#[test]
fn a_line_the_last_commits_list_lacks_fails_the_list_only_shrinks() {
    let scratch = cycle_of_two("added");
    scratch.commit();
    scratch.write(
        "koyo-rs/koyo/src/a.rs",
        "use crate::b::B;\nuse crate::c::C;\n",
    );
    scratch.write("koyo-rs/koyo/src/c.rs", "use crate::a::A;\n");
    scratch.write(LIST, "a b\nb a\na c\nc a\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        [
            "koyo-rs/module-edges.txt:3:1: added edge: a -> c",
            "koyo-rs/module-edges.txt:4:1: added edge: c -> a",
        ]
    );
    assert!(run.stdout.contains(ONLY_SHRINKS), "{}", run.stdout);

    scratch.write("koyo-rs/koyo/src/a.rs", "use crate::b::B;\n");
    scratch.write("koyo-rs/koyo/src/b.rs", "");
    scratch.write("koyo-rs/koyo/src/c.rs", "");
    scratch.write(LIST, "a b\n");
    let shrunk = scratch.run(&["--prune"]);
    assert_eq!(
        (shrunk.code, shrunk.stdout.as_str()),
        (0, "Pruned 1 edge from koyo-rs/module-edges.txt.\n")
    );
}

#[test]
fn before_the_commit_that_adds_the_list_nothing_is_compared_with_it() {
    let scratch = cycle_of_two("first-commit");
    fs::remove_file(scratch.path(LIST)).expect("remove the list");
    scratch.commit();
    scratch.write(LIST, "a b\nb a\n");
    let run = scratch.run(&[]);
    assert_eq!((run.code, run.stdout.as_str()), (0, ""));
}

#[test]
fn the_tool_fails_with_2_when_it_cannot_do_its_job() {
    let missing = cycle_of_two("missing-list");
    fs::remove_file(missing.path(LIST)).expect("remove the list");
    let run = missing.run(&[]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr
            .contains("git checkout HEAD -- koyo-rs/module-edges.txt"),
        "{}",
        run.stderr
    );

    let malformed = cycle_of_two("malformed-list");
    malformed.write(LIST, "a b\na b c\n");
    let run = malformed.run(&[]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr.contains("koyo-rs/module-edges.txt:2"),
        "{}",
        run.stderr
    );

    let no_file = checkout(
        "no-file",
        &[("main.rs", "mod a;\nmod gone;\n"), ("a.rs", "")],
    );
    let run = no_file.run(&[]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr.contains("koyo-rs/koyo/src/main.rs:2"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("koyo-rs/koyo/src/gone.rs"),
        "{}",
        run.stderr
    );
}
