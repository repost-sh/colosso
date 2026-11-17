use crate::support::{CheckCase, assert_check_cases};

const ATTACHED: &[CheckCase] = &[
    CheckCase {
        name: "above-an-unsafe-block",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: nothing\n    unsafe { f() }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "multi-line-run",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: the pointer is valid\n    // and nothing else holds it.\n    unsafe { f() }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-an-unsafe-impl",
        file: "case.rs",
        source: "// SAFETY: the handle is only used on one thread.\nunsafe impl Send for Handle {}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-an-unsafe-fn",
        file: "case.rs",
        source: "// SAFETY: callers pass a live pointer.\nunsafe fn read(pointer: *const u8) -> u8 { *pointer }\n",
        findings: &[],
    },
    CheckCase {
        name: "blank-line-before-the-unsafe",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: nothing\n\n    unsafe { f() }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "an-attribute-between-the-run-and-the-item",
        file: "case.rs",
        source: "// SAFETY: one thread only.\n#[expect(unsafe_code, reason = \"the handle\")]\nunsafe impl Send for Handle {}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-a-let-whose-initializer-holds-the-unsafe",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: nothing\n    let value: u8 =\n        unsafe { f() };\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-a-method-in-a-chain",
        file: "case.rs",
        source: "fn main() {\n    names\n        .first()\n        // SAFETY: the symbol outlives its library.\n        .and_then(|address| keep(unsafe { *address }))\n        .unwrap_or_default();\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-a-closure-body-in-a-chain",
        file: "case.rs",
        source: "fn main() {\n    names\n        // SAFETY: an id, copied.\n        .map(|id| unsafe {\n            make(id)\n        })\n        .collect();\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-a-statement-in-a-macro-body",
        file: "case.rs",
        source: "wrap! {\n    impl Handler {\n        fn on_created(&self) {\n            // SAFETY: a window handle is a view.\n            let view: Option<&View> = unsafe { handle().as_ref() };\n            done();\n        }\n    }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "above-a-static-in-a-macro-body",
        file: "case.rs",
        source: "thread_local! {\n    // SAFETY: `new` makes a manager with no delegate.\n    static MANAGER: Manager = unsafe { Manager::new() };\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "first-thing-in-an-unsafe-block",
        file: "case.rs",
        source: "fn main() {\n    unsafe {\n        // SAFETY: nothing\n        f();\n    }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "two-runs-above-one-unsafe",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: first\n    // SAFETY: second\n    unsafe { f() }\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "a-doc-comment-between-the-run-and-the-unsafe",
        file: "case.rs",
        source: "// SAFETY: one thread only.\n/// The handle.\nunsafe impl Send for Handle {}\n",
        findings: &[],
    },
];

const REJECTED: &[CheckCase] = &[
    CheckCase {
        name: "above-safe-code",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: nothing here\n    f();\n}\n",
        findings: &["case.rs:2:5: safety: // SAFETY: nothing here"],
    },
    CheckCase {
        name: "after-the-unsafe-instead-of-before-it",
        file: "case.rs",
        source: "fn main() {\n    unsafe { f() }\n    // SAFETY: too late\n}\n",
        findings: &["case.rs:3:5: safety: // SAFETY: too late"],
    },
    CheckCase {
        name: "trailing-the-unsafe-on-its-own-line",
        file: "case.rs",
        source: "fn main() {\n    let x = unsafe { f() }; // SAFETY: not above it\n    g();\n}\n",
        findings: &["case.rs:2:29: safety: // SAFETY: not above it"],
    },
    CheckCase {
        name: "above-the-wrong-segment-of-a-chain",
        file: "case.rs",
        source: "fn main() {\n    names\n        // SAFETY: about the next call only\n        .map(|id| id)\n        .and_then(|id| unsafe { f(id) });\n}\n",
        findings: &["case.rs:3:9: safety: // SAFETY: about the next call only"],
    },
    CheckCase {
        name: "above-a-safe-statement-in-a-macro-body",
        file: "case.rs",
        source: "wrap! {\n    fn f() {\n        // SAFETY: nothing here\n        g();\n        unsafe { h() };\n    }\n}\n",
        findings: &["case.rs:3:9: safety: // SAFETY: nothing here"],
    },
    CheckCase {
        name: "above-a-safe-fn-whose-body-holds-the-unsafe",
        file: "case.rs",
        source: "// SAFETY: somewhere below\nfn main() {\n    g();\n    unsafe { f() }\n}\n",
        findings: &["case.rs:1:1: safety: // SAFETY: somewhere below"],
    },
    CheckCase {
        name: "above-a-safe-impl-whose-methods-hold-the-unsafe",
        file: "case.rs",
        source: "// SAFETY: somewhere below\nimpl Handle {\n    fn get(&self) -> u8 {\n        unsafe { f() }\n    }\n}\n",
        findings: &["case.rs:1:1: safety: // SAFETY: somewhere below"],
    },
    CheckCase {
        name: "run-first-line-is-the-only-one-reported",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: one\n    // two\n    // three\n    f();\n}\n",
        findings: &["case.rs:2:5: safety: // SAFETY: one"],
    },
];

const NOT_A_RUN: &[CheckCase] = &[
    CheckCase {
        name: "plain-comment-above-a-run",
        file: "case.rs",
        source: "// plain\n// SAFETY: fine\nunsafe impl Send for Handle {}\n",
        findings: &["case.rs:1:1: comment: // plain"],
    },
    CheckCase {
        name: "plain-comment-after-a-blank-line-is-not-the-run",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: fine\n\n    // a plain one\n    unsafe { f() }\n}\n",
        findings: &["case.rs:4:5: comment: // a plain one"],
    },
    CheckCase {
        name: "plain-comment-below-the-unsafe",
        file: "case.rs",
        source: "// SAFETY: fine\nunsafe impl Send for Handle {}\n// after\n",
        findings: &["case.rs:3:1: comment: // after"],
    },
    CheckCase {
        name: "a-block-comment-ends-the-run",
        file: "case.rs",
        source: "fn main() {\n    // SAFETY: fine\n    /* block */\n    unsafe { f() }\n}\n",
        findings: &["case.rs:3:5: comment: /* block */"],
    },
    CheckCase {
        name: "wrong-spelling",
        file: "case.rs",
        source: "fn main() {\n    // Safety: fine\n    //SAFETY: fine\n    unsafe { f() }\n}\n",
        findings: &[
            "case.rs:2:5: comment: // Safety: fine",
            "case.rs:3:5: comment: //SAFETY: fine",
        ],
    },
    CheckCase {
        name: "block-comment-spelling",
        file: "case.rs",
        source: "fn main() {\n    /* SAFETY: fine */\n    unsafe { f() }\n}\n",
        findings: &["case.rs:2:5: comment: /* SAFETY: fine */"],
    },
    CheckCase {
        name: "a-run-in-typescript-is-an-ordinary-comment",
        file: "case.ts",
        source: "// SAFETY: fine\nexport const a = 1;\n",
        findings: &["case.ts:1:1: comment: // SAFETY: fine"],
    },
];

#[test]
fn a_run_above_an_unsafe_is_clean() {
    assert_check_cases("safety-attached", ATTACHED);
}

#[test]
fn a_run_that_is_not_above_an_unsafe_is_a_safety_finding() {
    assert_check_cases("safety-rejected", REJECTED);
}

#[test]
fn only_a_run_of_line_comments_starting_with_safety_is_one() {
    assert_check_cases("safety-not-a-run", NOT_A_RUN);
}
