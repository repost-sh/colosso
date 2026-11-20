use std::fs;
use std::path::Path;

use crate::support::Scratch;

#[test]
fn a_folder_is_walked_for_rust_and_typescript_and_paths_print_as_given() {
    let scratch = Scratch::new("files-folder");
    scratch.write("sub/a.rs", "// a\n");
    scratch.write("sub/deeper/b.ts", "// b\n");
    scratch.write("sub/c.tsx", "// c\n");
    scratch.write("sub/notes.md", "// not source\n");
    scratch.write("sub/d.js", "// not checked\n");
    let run = scratch.run(&["sub"]);
    assert_eq!(
        run.findings(),
        [
            "sub/a.rs:1:1: comment: // a",
            "sub/c.tsx:1:1: comment: // c",
            "sub/deeper/b.ts:1:1: comment: // b",
        ]
    );
    let dotted = scratch.run(&["./sub/a.rs"]);
    assert_eq!(dotted.findings(), ["./sub/a.rs:1:1: comment: // a"]);
}

#[test]
fn findings_come_out_in_path_order_however_many_files_there_are() {
    let scratch = Scratch::new("files-order");
    let names: Vec<String> = (0..80)
        .map(|index| format!("dir{}/file{index:02}.rs", index % 7))
        .collect();
    for name in &names {
        scratch.write(name, "// c\n");
    }
    let mut expected: Vec<String> = names
        .iter()
        .map(|name| format!("{name}:1:1: comment: // c"))
        .collect();
    expected.sort();
    for _ in 0..5 {
        let run = scratch.run(&["."]);
        let shown: Vec<String> = run
            .findings()
            .iter()
            .map(|line| line.trim_start_matches("./").to_owned())
            .collect();
        assert_eq!(shown, expected);
    }
}

#[test]
fn only_the_files_named_are_checked() {
    let scratch = Scratch::new("files-named");
    scratch.write("a.rs", "// a\n");
    scratch.write("b.rs", "// b\n");
    let run = scratch.run(&["b.rs"]);
    assert_eq!(run.findings(), ["b.rs:1:1: comment: // b"]);
}

#[test]
fn check_mode_never_writes() {
    let scratch = Scratch::new("files-read-only");
    scratch.write("a.rs", "// a\nfn f() {}\n");
    scratch.run(&["a.rs"]);
    assert_eq!(scratch.read("a.rs"), "// a\nfn f() {}\n");
}

#[test]
fn a_path_that_does_not_exist_is_a_tool_failure() {
    let scratch = Scratch::new("files-missing");
    let run = scratch.run(&["nothing.rs"]);
    assert_eq!(run.code, 2);
    assert!(run.stdout.is_empty());
    assert!(run.stderr.contains("nothing.rs"), "{}", run.stderr);
}

#[test]
fn a_file_of_another_kind_is_a_tool_failure() {
    let scratch = Scratch::new("files-kind");
    scratch.write("notes.md", "// x\n");
    let run = scratch.run(&["notes.md"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("notes.md"), "{}", run.stderr);
}

#[test]
fn an_unreadable_file_is_reported_and_the_others_are_still_checked() {
    let scratch = Scratch::new("files-unreadable");
    scratch.write_bytes("a.rs", b"fn f() { let _ = \"\xff\"; }\n");
    scratch.write("b.rs", "// b\n");
    let run = scratch.run(&["a.rs", "b.rs"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.findings(), ["b.rs:1:1: comment: // b"]);
    assert!(run.stderr.contains("a.rs"), "{}", run.stderr);
}

#[test]
fn a_deletion_that_would_fuse_tokens_is_refused_and_the_other_files_are_fixed() {
    let scratch = Scratch::new("files-not-fixable");
    let fused = "fn main() { let x = 1/**/.0; }\n";
    scratch.write("a.rs", "// c\nfn main() {}\n");
    scratch.write("fused.rs", fused);
    let run = scratch.run(&["--fix", "a.rs", "fused.rs"]);
    assert_eq!(run.code, 2);
    assert_eq!(scratch.read("a.rs"), "fn main() {}\n");
    assert_eq!(scratch.read("fused.rs"), fused);
    assert_eq!(run.stdout, "Deleted 1 comment from 1 file.\n");
    assert!(
        run.stderr.contains("fused.rs: not fixable"),
        "{}",
        run.stderr
    );
}

#[test]
fn help_and_unknown_options_are_answered_without_checking_anything() {
    let scratch = Scratch::new("files-usage");
    let help = scratch.run(&["--help"]);
    assert_eq!(help.code, 0);
    assert!(
        help.stdout.starts_with("koyo-hygiene [--fix] [PATH...]"),
        "{}",
        help.stdout
    );
    let unknown = scratch.run(&["--wat"]);
    assert_eq!(unknown.code, 2);
    assert!(unknown.stderr.contains("--wat"), "{}", unknown.stderr);
}

fn git_checkout(name: &str) -> Scratch {
    let scratch = Scratch::new(name);
    scratch.write(".gitignore", "ignored.rs\n");
    scratch.write("koyo-rs/module-edges.txt", "");
    scratch.write("koyo-rs/blocking-debt.txt", "");
    scratch.write("koyo-rs/blocking-debt-browser-host.txt", "");
    scratch.write("koyo-rs/koyo/src/main.rs", "fn main() {}\n");
    scratch.write("koyo-rs/koyo-browser-host/src/lib.rs", "");
    scratch.write("koyo-rs/koyo/src/a.rs", "// a\n");
    scratch.write("koyo-rs/koyo/src/ignored.rs", "// ignored\n");
    scratch.write("koyo-rs/koyo/src/gone.rs", "// gone\n");
    scratch.write("koyo-rs/koyo-git/src/b.rs", "// b\n");
    scratch.write("koyo-rs/koyo-git/notes.md", "// not source\n");
    scratch.write("koyo-rs/vendor/v/src/lib.rs", "// vendored\n");
    scratch.write("koyo-js/c.ts", "// c\n");
    scratch.write("koyo-js/d.tsx", "// d\n");
    scratch.write("other/e.rs", "// outside\n");
    scratch.git(&["init", "-q"]);
    scratch.git(&["add", "-A"]);
    scratch.write("koyo-rs/koyo/src/untracked.rs", "// untracked\n");
    fs::remove_file(scratch.path("koyo-rs/koyo/src/gone.rs")).expect("remove a tracked file");
    scratch
}

const GIT_FINDINGS: [&str; 5] = [
    "koyo-js/c.ts:1:1: comment: // c",
    "koyo-js/d.tsx:1:1: comment: // d",
    "koyo-rs/koyo-git/src/b.rs:1:1: comment: // b",
    "koyo-rs/koyo/src/a.rs:1:1: comment: // a",
    "koyo-rs/koyo/src/untracked.rs:1:1: comment: // untracked",
];

#[test]
fn with_no_path_git_lists_the_files_and_paths_are_relative_to_the_checkout() {
    let scratch = git_checkout("files-git");
    let run = scratch.run(&[]);
    assert_eq!(run.findings(), GIT_FINDINGS);
    assert_eq!(run.code, 1);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
}

#[test]
fn with_no_path_the_checkout_is_found_from_a_folder_inside_it() {
    let scratch = git_checkout("files-git-inside");
    let run = scratch.run_in(Path::new("koyo-rs/koyo/src"), &[]);
    assert_eq!(run.findings(), GIT_FINDINGS);
}

#[test]
fn fix_with_no_path_rewrites_only_the_listed_files() {
    let scratch = git_checkout("files-git-fix");
    let run = scratch.run(&["--fix"]);
    assert_eq!(run.code, 0);
    assert_eq!(run.stdout, "Deleted 5 comments from 5 files.\n");
    assert_eq!(scratch.read("koyo-rs/koyo/src/a.rs"), "");
    assert_eq!(scratch.read("koyo-rs/vendor/v/src/lib.rs"), "// vendored\n");
    assert_eq!(scratch.read("other/e.rs"), "// outside\n");
    assert_eq!(scratch.read("koyo-rs/koyo/src/ignored.rs"), "// ignored\n");
}

#[test]
fn outside_a_git_checkout_with_no_path_is_a_tool_failure() {
    let scratch = Scratch::new("files-no-git");
    let run = scratch.run(&[]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("git"), "{}", run.stderr);
}
