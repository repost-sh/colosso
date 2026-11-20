use super::support::with_debt;
use crate::imports::support::{HOST, HOST_DEBT};

/// A checkout whose browser host's root declares `a` and launches, and whose `a.rs` blocks
/// once lent a `Blocking`, once not, and launches where it may not.
fn host(name: &str, list: &str) -> crate::support::Scratch {
    let scratch = with_debt(name, &[("main.rs", "")], "");
    scratch.write(
        &format!("{HOST}lib.rs"),
        "\
mod a;
mod blocking;
pub fn main() {
    blocking::at_launch(|may| a::run(may));
}
",
    );
    scratch.write(
        &format!("{HOST}a.rs"),
        "\
use crate::blocking::{self, Blocking};
pub fn run(_: Blocking<'_>) {
    std::fs::read(\"lent\");
}
pub fn later() {
    blocking::thread(\"x\", |_| std::fs::read(\"lent\"));
    std::fs::read(\"owed\");
    blocking::at_launch(|_| ());
}
",
    );
    scratch.write(&format!("{HOST}blocking.rs"), "");
    scratch.write(HOST_DEBT, list);
    scratch
}

/// The browser host's shipped build is walked from its root as the app's is, its calls held to
/// a list of its own, and `at_launch` its root's alone.
#[test]
fn the_browser_hosts_calls_are_held_to_its_own_list() {
    let unlisted = host("host-unlisted", "").run(&[]);
    assert_eq!(unlisted.code, 1);
    assert_eq!(
        unlisted.findings(),
        [
            "koyo-rs/koyo-browser-host/src/a.rs:7:5: blocking call: std::fs::read",
            "koyo-rs/koyo-browser-host/src/a.rs:8:5: launch read: crate::blocking::at_launch",
        ]
    );
    let listed = host("host-listed", "koyo-browser-host/src/a.rs 1\n").run(&[]);
    assert_eq!(
        listed.findings(),
        ["koyo-rs/koyo-browser-host/src/a.rs:8:5: launch read: crate::blocking::at_launch"]
    );
}

/// A list holds its own crate's files only: one of the app's on the browser host's is a
/// failure of the tool, not debt it allows.
#[test]
fn a_file_of_another_crate_on_a_list_fails_the_tool() {
    let run = host("host-wrong-list", "koyo/src/main.rs 1\n").run(&[]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr
            .contains("koyo/src/main.rs isn't in koyo-rs/koyo-browser-host/"),
        "{}",
        run.stderr
    );
}
