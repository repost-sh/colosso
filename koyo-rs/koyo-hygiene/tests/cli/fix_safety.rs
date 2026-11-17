use crate::support::{FixCase, Scratch, assert_fix_cases, deleted_summary};

const KEPT: &[FixCase] = &[
    FixCase {
        name: "plain-comments-around-an-attached-run-go-and-the-run-stays",
        file: "case.rs",
        before: "// plain above\n// SAFETY: fine\nunsafe impl Send for Handle {}\n// plain below\n",
        after: "// SAFETY: fine\nunsafe impl Send for Handle {}\n",
        deleted: 2,
    },
    FixCase {
        name: "a-multi-line-run-stays-whole",
        file: "case.rs",
        before: "fn main() {\n    f(); // gone\n    // SAFETY: the pointer is valid\n    // and nothing else holds it.\n    unsafe { g() }\n}\n",
        after: "fn main() {\n    f();\n    // SAFETY: the pointer is valid\n    // and nothing else holds it.\n    unsafe { g() }\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "a-plain-comment-after-a-blank-line-goes-and-the-run-stays",
        file: "case.rs",
        before: "fn main() {\n    // SAFETY: fine\n\n    // plain\n    unsafe { g() }\n}\n",
        after: "fn main() {\n    // SAFETY: fine\n\n    unsafe { g() }\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "two-adjacent-runs-stay",
        file: "case.rs",
        before: "fn main() {\n    // SAFETY: first\n    // SAFETY: second\n    unsafe { g() }\n}\n",
        after: "fn main() {\n    // SAFETY: first\n    // SAFETY: second\n    unsafe { g() }\n}\n",
        deleted: 0,
    },
    FixCase {
        name: "a-file-of-runs-and-doc-comments-is-untouched",
        file: "case.rs",
        before: "//! docs\n// SAFETY: fine\nunsafe impl Send for Handle {}\n/// more\nfn f() {}\n",
        after: "//! docs\n// SAFETY: fine\nunsafe impl Send for Handle {}\n/// more\nfn f() {}\n",
        deleted: 0,
    },
];

#[test]
fn a_safety_run_is_never_deleted_or_moved() {
    assert_fix_cases("safety-kept", KEPT);
}

#[test]
fn an_unattached_run_stays_and_is_reported_after_the_fix() {
    let scratch = Scratch::new("safety-unattached");
    scratch.write(
        "case.rs",
        "// plain\n// plain\nfn main() {\n    // SAFETY: not above an unsafe\n    // second line\n    f(); // plain\n}\n",
    );
    let run = scratch.run(&["--fix", "case.rs"]);
    assert_eq!(
        scratch.read("case.rs"),
        "fn main() {\n    // SAFETY: not above an unsafe\n    // second line\n    f();\n}\n"
    );
    assert_eq!(run.code, 1);
    assert_eq!(run.findings()[0], deleted_summary(3, 1).trim_end());
    assert_eq!(
        run.findings()[1],
        "case.rs:2:5: safety: // SAFETY: not above an unsafe"
    );
    assert_eq!(run.findings().len(), 2);
    assert!(run.stderr.is_empty());
    assert_eq!(
        run.stdout
            .matches("A // SAFETY: comment argues for one unsafe")
            .count(),
        1
    );
    assert!(!run.stdout.contains("Koyo's source has no comments"));
}

#[test]
fn a_trailing_run_is_left_where_it_is_and_reported() {
    let scratch = Scratch::new("safety-trailing");
    let source = "fn main() {\n    let x = unsafe { f() }; // SAFETY: not above it\n}\n";
    scratch.write("case.rs", source);
    let run = scratch.run(&["--fix", "case.rs"]);
    assert_eq!(scratch.read("case.rs"), source);
    assert_eq!(run.code, 1);
    assert!(
        run.stdout
            .contains("case.rs:2:29: safety: // SAFETY: not above it")
    );
}
