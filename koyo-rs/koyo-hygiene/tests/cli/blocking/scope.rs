use super::support::{CallCase, assert_call_cases, calls, with_debt};

const ROOT: &str = "mod a;\nmod blocking;\n";

#[test]
fn a_call_in_a_function_that_takes_a_blocking_is_lent_and_one_without_is_owed() {
    assert_call_cases(&[CallCase {
        name: "takes-a-token",
        files: &[
            ("main.rs", ROOT),
            ("blocking.rs", ""),
            (
                "a.rs",
                "\
use std::fs;
use crate::blocking::{self, Blocking};
fn lent(path: &Path, _: Blocking<'_>) -> bool {
    fs::read(path).is_ok() && path.exists()
}
fn by_reference(may: &crate::blocking::Blocking<'_>) {
    let _ = fs::read(\"x\");
}
fn owed(path: &Path) -> bool {
    fs::read(path).is_ok()
}
",
            ),
        ],
        calls: &["a.rs:10:5: std::fs::read"],
    }]);
}

#[test]
fn a_call_in_a_closure_a_maker_lends_to_is_lent_nested_closures_too() {
    assert_call_cases(&[CallCase {
        name: "maker-closures",
        files: &[
            ("main.rs", ROOT),
            ("blocking.rs", ""),
            (
                "a.rs",
                "\
use crate::blocking::{self, in_test};
fn f(cx: &mut App) {
    blocking::spawn(cx.background_executor(), move |_| std::fs::read(\"x\"));
    blocking::thread(\"watch\", |_| {
        let read = |path| std::fs::read(path);
        loop { read(\"y\"); }
    });
    blocking::on_quit(cx, |_, _| std::fs::write(\"z\", \"\"));
    in_test(|_| std::fs::read(\"t\"));
    let typed = |_: crate::blocking::Blocking<'_>| std::fs::read(\"u\");
}
",
            ),
        ],
        calls: &[],
    }]);
}

#[test]
fn a_closure_another_function_takes_and_a_function_inside_a_lent_one_are_owed() {
    assert_call_cases(&[CallCase {
        name: "not-lent",
        files: &[
            ("main.rs", ROOT),
            ("blocking.rs", ""),
            (
                "a.rs",
                "\
use crate::blocking::Blocking;
fn f(cx: &mut App, may: Blocking<'_>) {
    fn inner() {
        std::fs::read(\"x\");
    }
    cx.spawn(async move |_| std::fs::read(\"y\")).detach();
}
fn g(cx: &mut App) {
    cx.defer(|_| std::fs::read(\"z\"));
    blocking::spawn_soon(|_| std::fs::read(\"w\"));
}
",
            ),
        ],
        calls: &[
            "a.rs:4:9: std::fs::read",
            "a.rs:9:18: std::fs::read",
            "a.rs:10:30: std::fs::read",
        ],
    }]);
}

#[test]
fn the_list_counts_only_the_calls_no_blocking_is_lent_to() {
    let files = &[
        ("main.rs", ROOT),
        ("blocking.rs", ""),
        (
            "a.rs",
            "\
use crate::blocking::{self, Blocking};
fn lent(_: Blocking<'_>) {
    std::fs::read(\"x\");
    std::fs::read(\"y\");
}
fn owed(cx: &App) {
    std::fs::read(\"z\");
    blocking::spawn(cx.background_executor(), |_| std::fs::read(\"w\"));
}
",
        ),
    ];
    let allowed = with_debt("counted", files, "koyo/src/a.rs 1\n").run(&[]);
    assert_eq!((allowed.code, allowed.stdout.as_str()), (0, ""));
    let stats = with_debt("counted-stats", files, "koyo/src/a.rs 1\n").run(&["--stats"]);
    assert!(
        stats.stdout.ends_with(
            "Blocking calls: 3 on a Blocking, 1 owed in 1 file (koyo-rs/blocking-debt.txt), 0 at_launch outside main.rs.\n"
        ),
        "{}",
        stats.stdout
    );
    let none = with_debt("counted-none", files, "").run(&[]);
    assert_eq!(calls(&none), ["a.rs:7:5: std::fs::read"]);
}
