use crate::support::Scratch;

const COMMENT_FIX_FIRST_LINE: &str =
    "Koyo's source has no comments. Delete each one and say it where a program checks it:";
const FIX_COMMAND: &str = "script/cargo run -q -p koyo-hygiene -- --fix";
const SAFETY_FIX_FIRST_LINE: &str =
    "A // SAFETY: comment argues for one unsafe block or impl and sits directly above it.";

#[test]
fn the_fix_text_appears_once_however_many_findings_there_are() {
    let scratch = Scratch::new("output-once");
    scratch.write("case.rs", "// a\nfn f() {}\n// b\nfn g() {} // c\n");
    let run = scratch.run(&["case.rs"]);
    assert_eq!(run.code, 1);
    assert_eq!(run.stdout.matches(COMMENT_FIX_FIRST_LINE).count(), 1);
    assert_eq!(run.stdout.matches(FIX_COMMAND).count(), 1);
    assert!(!run.stdout.contains(SAFETY_FIX_FIRST_LINE));
}

#[test]
fn the_comment_fix_text_names_each_replacement_and_the_fix_command() {
    let scratch = Scratch::new("output-comment-text");
    scratch.write("case.rs", "// a\nfn f() {}\n");
    let run = scratch.run(&["case.rs"]);
    let expected_tail = "\
Koyo's source has no comments. Delete each one and say it where a program checks it:
  what the code does             a clearer name for the function, variable or type
  a condition that must hold     a type that cannot hold the wrong value, or a test named for it
  why a lint is silenced         #[expect(the_lint, reason = \"...\")]
  why an unsafe block is sound   // SAFETY: ... directly above the unsafe, the one comment allowed
To delete them all: script/cargo run -q -p koyo-hygiene -- --fix
";
    assert!(run.stdout.ends_with(expected_tail), "{}", run.stdout);
}

#[test]
fn a_count_line_follows_the_findings_and_names_each_rule() {
    let scratch = Scratch::new("output-count");
    scratch.write(
        "case.rs",
        "// a\nfn f() {\n    // SAFETY: not above an unsafe\n    g();\n}\n/* b */\n",
    );
    let run = scratch.run(&["case.rs"]);
    assert_eq!(
        run.findings(),
        [
            "case.rs:1:1: comment: // a",
            "case.rs:3:5: safety: // SAFETY: not above an unsafe",
            "case.rs:6:1: comment: /* b */",
        ]
    );
    assert!(
        run.stdout
            .contains("\n3 findings in 1 file (2 comment, 1 safety).\n"),
        "{}",
        run.stdout
    );
    assert_eq!(run.stdout.matches(COMMENT_FIX_FIRST_LINE).count(), 1);
    assert_eq!(run.stdout.matches(SAFETY_FIX_FIRST_LINE).count(), 1);
}

#[test]
fn a_clean_file_prints_nothing_and_exits_zero() {
    let scratch = Scratch::new("output-clean");
    scratch.write("case.rs", "/// doc\nfn f() {}\n");
    let run = scratch.run(&["case.rs"]);
    assert_eq!(
        (run.code, run.stdout.as_str(), run.stderr.as_str()),
        (0, "", "")
    );
}

#[test]
fn the_excerpt_is_the_first_line_trimmed_and_cut_near_eighty_characters() {
    let scratch = Scratch::new("output-excerpt");
    let long = "x".repeat(120);
    scratch.write(
        "case.rs",
        &format!("//    {long}\nfn f() {{}}\n/* first line   \n second */\n"),
    );
    let run = scratch.run(&["case.rs"]);
    assert_eq!(
        run.findings(),
        [
            format!("case.rs:1:1: comment: //    {}...", "x".repeat(74)),
            "case.rs:3:1: comment: /* first line".to_owned(),
        ]
    );
}
