use crate::support::{FixCase, assert_fix_cases};

const CASES: &[FixCase] = &[
    FixCase {
        name: "never-two-blank-lines-in-a-row",
        file: "case.rs",
        before: "fn f() {}\n\n// c\n\nfn g() {}\n",
        after: "fn f() {}\n\nfn g() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "several-comments-between-blank-lines-leave-one",
        file: "case.rs",
        before: "fn f() {}\n\n// a\n\n// b\n\nfn g() {}\n",
        after: "fn f() {}\n\nfn g() {}\n",
        deleted: 2,
    },
    FixCase {
        name: "no-blank-line-after-an-open-brace",
        file: "case.rs",
        before: "fn f() {\n    // c\n\n    a();\n}\n",
        after: "fn f() {\n    a();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-after-an-open-paren",
        file: "case.rs",
        before: "fn f() {\n    g(\n        // c\n\n        1,\n    );\n}\n",
        after: "fn f() {\n    g(\n        1,\n    );\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-after-an-open-bracket",
        file: "case.rs",
        before: "fn f() {\n    let v = [\n        // c\n\n        1,\n    ];\n}\n",
        after: "fn f() {\n    let v = [\n        1,\n    ];\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-before-a-close-brace",
        file: "case.rs",
        before: "fn f() {\n    a();\n\n    // c\n}\n",
        after: "fn f() {\n    a();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-before-a-close-paren",
        file: "case.rs",
        before: "fn f() {\n    g(\n        1,\n\n        // c\n    );\n}\n",
        after: "fn f() {\n    g(\n        1,\n    );\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-before-a-close-bracket",
        file: "case.rs",
        before: "fn f() {\n    let v = [\n        1,\n\n        // c\n    ];\n}\n",
        after: "fn f() {\n    let v = [\n        1,\n    ];\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-at-the-start-of-the-file",
        file: "case.rs",
        before: "// c\n\nfn f() {}\n",
        after: "fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-lines-at-the-start-after-a-block-comment",
        file: "case.rs",
        before: "/* c */\n\n\nfn f() {}\n",
        after: "fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "no-blank-line-at-the-end-of-the-file",
        file: "case.rs",
        before: "fn f() {}\n\n// c\n",
        after: "fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "blank-lines-either-side-of-a-lone-comment-in-a-block",
        file: "case.rs",
        before: "fn f() {\n\n    // c\n\n}\n",
        after: "fn f() {\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "a-blank-line-that-separates-statements-stays",
        file: "case.rs",
        before: "fn f() {\n    a();\n\n    // c\n    b();\n}\n",
        after: "fn f() {\n    a();\n\n    b();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "a-blank-line-above-a-removed-comment-stays-when-code-follows",
        file: "case.rs",
        before: "fn f() {}\n\n// c\nfn g() {}\n",
        after: "fn f() {}\n\nfn g() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "blank-lines-the-removal-did-not-create-stay",
        file: "case.rs",
        before: "// c\nfn f() {\n\n    a();\n\n\n    b();\n\n}\n",
        after: "fn f() {\n\n    a();\n\n\n    b();\n\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "a-trailing-comment-leaves-the-blank-lines-around-it",
        file: "case.rs",
        before: "fn f() {\n\n    a(); // c\n\n\n    b();\n\n}\n",
        after: "fn f() {\n\n    a();\n\n\n    b();\n\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "multi-line-block-comment-between-blank-lines",
        file: "case.rs",
        before: "fn f() {}\n\n/* a\n   b\n   c */\n\nfn g() {}\n",
        after: "fn f() {}\n\nfn g() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "typescript-brace-rules",
        file: "case.ts",
        before: "export function f() {\n  // c\n\n  return 1;\n\n  // d\n}\n",
        after: "export function f() {\n  return 1;\n}\n",
        deleted: 2,
    },
    FixCase {
        name: "crlf-blank-lines",
        file: "case.rs",
        before: "fn f() {\r\n    a();\r\n\r\n    // c\r\n}\r\n",
        after: "fn f() {\r\n    a();\r\n}\r\n",
        deleted: 1,
    },
];

#[test]
fn removing_comments_leaves_no_blank_line_that_only_separated_them() {
    assert_fix_cases("blank", CASES);
}
