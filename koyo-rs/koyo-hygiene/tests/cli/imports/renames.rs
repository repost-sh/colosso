use std::fs;

use super::support::{LIST, SOURCE, checkout};
use crate::support::{Run, Scratch};

/// Lines enough that a file holding them stays the same file through an edit, `of` telling it
/// from any other.
fn body(of: &str) -> String {
    let functions: Vec<String> = (0..12)
        .map(|index| format!("fn {of}_{index}() -> u32 {{\n    {index}\n}}\n"))
        .collect();
    functions.concat()
}

fn importing(of: &str, imports: &str) -> String {
    format!("{imports}\n{}", body(of))
}

fn source(file: &str) -> String {
    format!("{SOURCE}{file}")
}

fn write(scratch: &Scratch, files: &[(&str, &str)]) {
    for (file, text) in files {
        scratch.write(&source(file), text);
    }
}

/// The lines of the list the run reports as added, as `from -> to`.
fn added(run: &Run) -> Vec<&str> {
    run.findings()
        .iter()
        .filter_map(|line| Some(line.split_once(": added edge: ")?.1))
        .collect()
}

fn assert_clean(run: &Run, name: &str) {
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", ""),
        "{name}"
    );
}

/// The list of [`committed`].
const LISTED: &str = "a b\nb c\nb d\nb e\nc a\nd b\ne a\n";

/// A commit of the cycles `a -> b -> c -> a`, `a -> b -> e -> a` and `b -> d -> b`, all on its
/// list, whose `a -> b` is the import `a/x.rs` makes.
fn committed(name: &str) -> Scratch {
    let scratch = checkout(
        &format!("renames-{name}"),
        &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\nmod e;\n"),
            ("a.rs", "mod x;\n"),
            ("a/x.rs", &importing("x", "use crate::b::B;")),
            (
                "b.rs",
                &importing("b", "use crate::c::C;\nuse crate::d::D;\nuse crate::e::E;"),
            ),
            ("c.rs", "use crate::a::A;\n"),
            ("d.rs", "use crate::b::B;\n"),
            ("e.rs", "use crate::a::A;\n"),
        ],
    );
    scratch.write(LIST, LISTED);
    assert_clean(&scratch.run(&[]), name);
    scratch.commit();
    scratch
}

/// Moves `a/x.rs` to `to`, a module of `c`, and the list's line with it.
fn x_moved_to(scratch: &Scratch, to: &str) {
    scratch.rename(&source("a/x.rs"), &source(to));
    write(
        scratch,
        &[
            ("a.rs", ""),
            ("c.rs", &format!("#[path = \"{to}\"]\nmod x;\n")),
        ],
    );
    scratch.write(LIST, "b c\nb d\nc b\nd b\n");
}

#[test]
fn an_import_that_moved_with_its_file_keeps_its_line() {
    for (name, staged, edit) in [
        ("untracked", false, false),
        ("untracked-edited", false, true),
        ("staged", true, false),
        ("staged-edited", true, true),
    ] {
        let scratch = committed(name);
        x_moved_to(&scratch, "c/x.rs");
        if staged {
            scratch.git(&["add", "-A"]);
        }
        if edit {
            let edited =
                importing("x", "use crate::b::{self, Other};").replace("{\n    3", "{\n    4");
            scratch.write(&source("c/x.rs"), &edited);
        }
        assert_clean(&scratch.run(&[]), name);
    }
}

#[test]
fn a_file_by_any_name_keeps_its_lines() {
    let scratch = committed("names");
    scratch.rename(&source("a/x.rs"), &source("a/old ñame.rs"));
    write(
        &scratch,
        &[("a.rs", "#[path = \"a/old ñame.rs\"]\nmod x;\n")],
    );
    scratch.commit();
    scratch.rename(&source("a/old ñame.rs"), &source("a/x.rs"));
    x_moved_to(&scratch, "c/größe 名.rs");
    assert_clean(&scratch.run(&[]), "names");
}

#[test]
fn a_new_import_in_a_file_of_the_module_that_did_not_move_fails() {
    let scratch = committed("unmoved");
    write(
        &scratch,
        &[
            (
                "main.rs",
                "mod a;\nmod b;\nmod c;\nmod d;\nmod e;\nmod f;\n",
            ),
            (
                "a/x.rs",
                &importing("x", "use crate::b::B;\nuse crate::f::F;"),
            ),
            ("f.rs", ""),
        ],
    );
    scratch.commit();
    x_moved_to(&scratch, "c/x.rs");
    write(
        &scratch,
        &[
            ("c.rs", "mod x;\nuse crate::b::B;\n"),
            ("c/x.rs", &importing("x", "use crate::f::F;")),
        ],
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        ["koyo-rs/module-edges.txt:3:1: added edge: c -> b"]
    );

    scratch.write(LIST, "b c\nb d\nd b\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        ["koyo-rs/koyo/src/c.rs:2:12: new edge: c -> b closes c -> b -> c"]
    );
}

#[test]
fn a_moved_file_that_takes_up_an_import_of_one_it_left_fails() {
    let scratch = committed("takes-up");
    write(
        &scratch,
        &[
            ("a.rs", "mod x;\nmod y;\n"),
            ("a/x.rs", &importing("x", "")),
            ("a/y.rs", &importing("y", "use crate::b::B;")),
        ],
    );
    assert_clean(&scratch.run(&[]), "takes-up");
    scratch.commit();
    x_moved_to(&scratch, "c/x.rs");
    write(
        &scratch,
        &[
            ("a.rs", "mod y;\n"),
            ("a/y.rs", &body("y")),
            ("c/x.rs", &importing("x", "use crate::b::B;")),
        ],
    );
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["c -> b"]);
}

#[test]
fn a_moved_file_that_gains_an_import_that_closes_a_cycle_fails() {
    let scratch = committed("gains");
    x_moved_to(&scratch, "c/x.rs");
    scratch.write(
        &source("c/x.rs"),
        &importing("x", "use crate::b::B;\nuse crate::d::D;"),
    );
    scratch.write(LIST, "b c\nb d\nc b\nc d\nd b\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(
        run.findings(),
        ["koyo-rs/module-edges.txt:4:1: added edge: c -> d"]
    );
}

#[test]
fn an_import_that_moved_to_another_module_of_the_file_it_was_in_fails() {
    let scratch = checkout(
        "renames-inline",
        &[
            ("main.rs", "mod a {\n    use crate::b::B;\n}\nmod b;\n"),
            ("b.rs", "use crate::a::A;\nuse crate::init;\n"),
        ],
    );
    scratch.write(LIST, "a b\nb a\n");
    assert_clean(&scratch.run(&[]), "inline");
    scratch.commit();
    write(
        &scratch,
        &[("main.rs", "mod a {}\nmod b;\nuse crate::b::B;\n")],
    );
    scratch.write(LIST, "b main\nmain b\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["b -> main", "main -> b"]);
}

#[test]
fn an_import_the_list_did_not_hold_has_no_line_to_keep() {
    let scratch = committed("off-cycle");
    scratch.write(
        &source("a/x.rs"),
        &importing("x", "use crate::b::B;\nuse crate::f::F;"),
    );
    write(
        &scratch,
        &[
            (
                "main.rs",
                "mod a;\nmod b;\nmod c;\nmod d;\nmod e;\nmod f;\n",
            ),
            ("f.rs", ""),
        ],
    );
    assert_clean(&scratch.run(&[]), "off-cycle");
    scratch.commit();
    x_moved_to(&scratch, "c/x.rs");
    scratch.write(&source("f.rs"), "use crate::b::B;\n");
    scratch.write(LIST, "b c\nb d\nc b\nc f\nd b\nf b\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["c -> f", "f -> b"]);
}

#[test]
fn a_copy_of_a_file_takes_no_line_with_it() {
    let scratch = committed("copy");
    write(
        &scratch,
        &[
            ("c.rs", "mod x;\nuse crate::a::A;\n"),
            ("c/x.rs", &importing("x", "use crate::b::B;")),
        ],
    );
    scratch.write(LIST, "a b\nb c\nb d\nb e\nc a\nc b\nd b\ne a\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["c -> b"]);
}

#[test]
fn a_line_still_on_the_list_is_not_kept_twice() {
    let scratch = committed("still-listed");
    write(
        &scratch,
        &[
            ("a.rs", "mod x;\nmod y;\n"),
            ("a/y.rs", &importing("y", "use crate::b::B;")),
        ],
    );
    scratch.commit();
    scratch.rename(&source("a/x.rs"), &source("c/x.rs"));
    write(
        &scratch,
        &[("a.rs", "mod y;\n"), ("c.rs", "mod x;\nuse crate::a::A;\n")],
    );
    scratch.write(LIST, "a b\nb c\nb d\nb e\nc a\nc b\nd b\ne a\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["c -> b"]);
}

#[test]
fn two_lines_do_not_both_take_the_place_of_one() {
    let scratch = committed("one-line");
    write(
        &scratch,
        &[
            ("a.rs", "mod x;\nmod y;\n"),
            ("a/y.rs", &importing("y", "use crate::b::B;")),
        ],
    );
    scratch.commit();
    scratch.rename(&source("a/x.rs"), &source("c/x.rs"));
    scratch.rename(&source("a/y.rs"), &source("e/y.rs"));
    write(
        &scratch,
        &[("a.rs", ""), ("c.rs", "mod x;\n"), ("e.rs", "mod y;\n")],
    );
    scratch.write(LIST, "b c\nb d\nb e\nc b\nd b\ne b\n");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 1);
    assert_eq!(added(&run), ["e -> b"]);
}

#[test]
fn each_line_is_paired_with_one_that_went_where_any_pairing_does() {
    let scratch = committed("pairing");
    write(
        &scratch,
        &[
            ("a.rs", "mod x;\nmod y;\n"),
            ("a/y.rs", &importing("y", "use crate::b::B;")),
            ("d.rs", "mod z;\n"),
            ("d/z.rs", &importing("z", "use crate::b::B;")),
        ],
    );
    assert_clean(&scratch.run(&[]), "pairing");
    scratch.commit();
    scratch.rename(&source("a/x.rs"), &source("c/x.rs"));
    scratch.rename(&source("a/y.rs"), &source("e/y.rs"));
    scratch.rename(&source("d/z.rs"), &source("c/z.rs"));
    write(
        &scratch,
        &[
            ("a.rs", ""),
            ("c.rs", "mod x;\nmod z;\n"),
            ("d.rs", ""),
            ("e.rs", "mod y;\n"),
        ],
    );
    scratch.write(LIST, "b c\nb e\nc b\ne b\n");
    assert_clean(&scratch.run(&[]), "pairing");
}

#[test]
fn a_renamed_module_keeps_the_lines_of_the_edges_from_it_and_to_it() {
    let scratch = committed("module");
    scratch.rename(&source("b.rs"), &source("z/mod.rs"));
    write(
        &scratch,
        &[
            ("main.rs", "mod a;\nmod c;\nmod d;\nmod e;\nmod z;\n"),
            ("a/x.rs", &importing("x", "use crate::z::B;")),
            ("d.rs", "use crate::z::B;\n"),
        ],
    );
    scratch.write(LIST, "a z\nc a\nd z\ne a\nz c\nz d\nz e\n");
    assert_clean(&scratch.run(&[]), "module");
}

/// A commit whose `b` makes `import` and whose `a/look.rs` imports `b`; then `a/look.rs` moved to
/// `c`, with `b` making `import_now` in place of its own.
fn look_moved(name: &str, import: &str, import_now: &str) -> Run {
    let scratch = checkout(
        &format!("renames-{name}"),
        &[
            ("main.rs", "mod a;\nmod b;\nmod c;\n"),
            ("a.rs", "pub mod look;\npub mod kept;\n"),
            ("a/kept.rs", ""),
            ("a/look.rs", &importing("look", "use crate::b::B;")),
            ("b.rs", &format!("use crate::{import};\n")),
            ("c.rs", "use crate::b::B;\n"),
        ],
    );
    scratch.write(LIST, "a b\nb a\n");
    assert_clean(&scratch.run(&[]), name);
    scratch.commit();
    scratch.rename(&source("a/look.rs"), &source("c/look.rs"));
    write(
        &scratch,
        &[
            ("a.rs", "pub mod kept;\n"),
            ("b.rs", &format!("use crate::{import_now};\n")),
            ("c.rs", "pub mod look;\npub mod other;\nuse crate::b::B;\n"),
            ("c/other.rs", ""),
        ],
    );
    scratch.write(LIST, "b c\nc b\n");
    scratch.run(&[])
}

const LOOK: &str = "{a::{kept::K, look::{Look, Tone}}}";

#[test]
fn an_import_of_a_file_that_moved_keeps_its_line_where_it_is_still_of_that_file() {
    for (name, now) in [
        ("look-path", "c::look::Look"),
        ("look-list", "{c::{other::O, look::{self, Tone}}}"),
        ("look-glob", "c::look::*"),
    ] {
        let run = look_moved(name, LOOK, now);
        assert_eq!((run.code, added(&run)), (0, Vec::new()), "{name}");
    }
}

#[test]
fn an_import_of_the_module_a_file_moved_to_but_not_of_the_file_fails() {
    for (name, now) in [
        ("look-other", "c::other::Other"),
        ("look-module", "c::Look"),
        ("look-all", "c::*"),
        ("look-listed", "c::{other::{look::Look}}"),
    ] {
        let run = look_moved(name, LOOK, now);
        assert_eq!((run.code, added(&run)), (1, vec!["b -> c"]), "{name}");
    }
}

#[test]
fn an_import_of_a_file_that_moved_by_one_that_did_not_import_it_fails() {
    for (name, before) in [
        ("look-kept", "a::kept::K"),
        ("look-kept-listed", "a::{kept::{look::K},}"),
        ("look-of-module", "a::Look"),
    ] {
        let run = look_moved(name, before, "c::look::Look");
        assert_eq!((run.code, added(&run)), (1, vec!["b -> c"]), "{name}");
    }
}

#[test]
fn an_import_of_a_file_a_renamed_module_gained_fails() {
    let scratch = committed("gained-file");
    scratch.rename(&source("b.rs"), &source("z.rs"));
    write(
        &scratch,
        &[
            ("main.rs", "mod a;\nmod c;\nmod d;\nmod e;\nmod z;\n"),
            ("a/x.rs", &importing("x", "use crate::z::{new::B,};")),
            ("d.rs", "use crate::z::B;\n"),
            (
                "z.rs",
                &importing(
                    "b",
                    "mod new;\nuse crate::c::C;\nuse crate::d::D;\nuse crate::e::E;",
                ),
            ),
            ("z/new.rs", ""),
        ],
    );
    scratch.write(LIST, "a z\nc a\nd z\ne a\nz c\nz d\nz e\n");
    let run = scratch.run(&[]);
    assert_eq!((run.code, added(&run)), (1, vec!["a -> z"]));
}

#[test]
fn prune_keeps_the_lines_that_moved_and_takes_off_those_they_left() {
    let scratch = committed("prune");
    x_moved_to(&scratch, "c/x.rs");
    scratch.write(LIST, "a b\nb c\nb d\nb e\nc a\nc b\nd b\ne a\n");
    let run = scratch.run(&["--prune"]);
    assert_eq!(
        (run.code, run.stdout.as_str()),
        (0, "Pruned 4 edges from koyo-rs/module-edges.txt.\n")
    );
    assert_eq!(scratch.read(LIST), "b c\nb d\nc b\nd b\n");
}

#[test]
fn before_the_commit_that_adds_the_list_a_moved_import_is_compared_with_nothing() {
    let scratch = committed("first-commit");
    fs::remove_file(scratch.path(LIST)).expect("remove the list");
    scratch.commit();
    x_moved_to(&scratch, "c/x.rs");
    write(&scratch, &[("c.rs", "mod x;\nuse crate::b::B;\n")]);
    assert_clean(&scratch.run(&[]), "first-commit");
}
