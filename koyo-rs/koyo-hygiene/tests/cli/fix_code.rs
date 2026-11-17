use crate::support::{FixCase, assert_fix_cases};

const RUST: &[FixCase] = &[
    FixCase {
        name: "whole-line-comment",
        file: "case.rs",
        before: "// a\nfn main() {}\n",
        after: "fn main() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "indented-whole-line-comment",
        file: "case.rs",
        before: "fn main() {\n    // a\n    f();\n}\n",
        after: "fn main() {\n    f();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "trailing-comment-takes-the-space-before-it",
        file: "case.rs",
        before: "fn main() {\n    f();     // a\n}\n",
        after: "fn main() {\n    f();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "trailing-comment-takes-the-space-after-it",
        file: "case.rs",
        before: "fn main() {\n    f(); /* a */   \n}\n",
        after: "fn main() {\n    f();\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "multi-line-block-comment-on-its-own-lines",
        file: "case.rs",
        before: "/* a\n   b */\nfn main() {}\n",
        after: "fn main() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "nested-block-comment",
        file: "case.rs",
        before: "/* a /* b */ c */\nfn main() {}\n",
        after: "fn main() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-with-spaces-on-both-sides",
        file: "case.rs",
        before: "fn main() {\n    let a = /* c */ 1;\n}\n",
        after: "fn main() {\n    let a = 1;\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-keeps-two-words-apart",
        file: "case.rs",
        before: "pub/**/fn f() {}\n",
        after: "pub fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-before-a-word-keeps-one-space",
        file: "case.rs",
        before: "pub /* c */fn f() {}\n",
        after: "pub fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-after-a-word-keeps-one-space",
        file: "case.rs",
        before: "pub/* c */ fn f() {}\n",
        after: "pub fn f() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-between-brackets-leaves-no-space",
        file: "case.rs",
        before: "fn main(/* nothing */) {}\n",
        after: "fn main() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-keeps-two-operators-apart",
        file: "case.rs",
        before: "fn main() {\n    let a = 1 -/**/-1;\n}\n",
        after: "fn main() {\n    let a = 1 - -1;\n}\n",
        deleted: 1,
    },
    FixCase {
        name: "block-comment-leading-a-line-of-code",
        file: "case.rs",
        before: "/* a */ fn main() {}\nfn g() {\n    /* b */ h();\n}\n",
        after: "fn main() {}\nfn g() {\n    h();\n}\n",
        deleted: 2,
    },
    FixCase {
        name: "two-comments-on-one-line",
        file: "case.rs",
        before: "fn main() {\n    /* a */ /* b */\n    f(); /* c */ /* d */\n}\n",
        after: "fn main() {\n    f();\n}\n",
        deleted: 4,
    },
    FixCase {
        name: "block-then-line-comment-on-one-line",
        file: "case.rs",
        before: "/* a */ // b\nfn main() {}\n",
        after: "fn main() {}\n",
        deleted: 2,
    },
    FixCase {
        name: "comment-on-the-last-line-without-a-newline",
        file: "case.rs",
        before: "fn main() {}\n// last",
        after: "fn main() {}\n",
        deleted: 1,
    },
    FixCase {
        name: "trailing-comment-on-the-last-line-without-a-newline",
        file: "case.rs",
        before: "fn main() {} // last",
        after: "fn main() {}",
        deleted: 1,
    },
    FixCase {
        name: "a-file-of-only-comments-becomes-empty",
        file: "case.rs",
        before: "// a\n// b\n/* c */\n",
        after: "",
        deleted: 3,
    },
    FixCase {
        name: "comments-in-a-macro-body",
        file: "case.rs",
        before: "m! {\n    // c\n    fn f() {} // d\n}\n",
        after: "m! {\n    fn f() {}\n}\n",
        deleted: 2,
    },
    FixCase {
        name: "slashes-in-strings-stay",
        file: "case.rs",
        before: "fn main() {\n    let a = \"// keep\"; // drop\n    let b = r#\"/* keep */\"#; /* drop */\n}\n",
        after: "fn main() {\n    let a = \"// keep\";\n    let b = r#\"/* keep */\"#;\n}\n",
        deleted: 2,
    },
    FixCase {
        name: "doc-comments-stay-and-plain-ones-go",
        file: "case.rs",
        before: "/// doc\n// plain\nfn main() {}\n//! inner\n/** outer */\n/*! inner */\n",
        after: "/// doc\nfn main() {}\n//! inner\n/** outer */\n/*! inner */\n",
        deleted: 1,
    },
    FixCase {
        name: "non-ascii-text-around-comments",
        file: "case.rs",
        before: "fn main() {\n    let s = \"é\"; // é\n    // ü\n}\n",
        after: "fn main() {\n    let s = \"é\";\n}\n",
        deleted: 2,
    },
    FixCase {
        name: "crlf-line-endings-stay",
        file: "case.rs",
        before: "fn main() {\r\n    f(); // a\r\n    // b\r\n    g();\r\n}\r\n",
        after: "fn main() {\r\n    f();\r\n    g();\r\n}\r\n",
        deleted: 2,
    },
    FixCase {
        name: "a-file-with-only-doc-comments-is-untouched",
        file: "case.rs",
        before: "//! inner\n/// outer\n/** block */\nfn main() {}\n",
        after: "//! inner\n/// outer\n/** block */\nfn main() {}\n",
        deleted: 0,
    },
    FixCase {
        name: "a-file-without-comments-is-untouched",
        file: "case.rs",
        before: "fn main() {\n\n\n    f();\n}\n\n\n",
        after: "fn main() {\n\n\n    f();\n}\n\n\n",
        deleted: 0,
    },
];

const TYPESCRIPT: &[FixCase] = &[
    FixCase {
        name: "line-block-and-trailing-comments",
        file: "case.ts",
        before: "// a\nexport const a = 1; // b\n/* c */\n/** d */\nexport const b = 2;\n",
        after: "export const a = 1;\n/** d */\nexport const b = 2;\n",
        deleted: 3,
    },
    FixCase {
        name: "block-comment-between-operands",
        file: "case.ts",
        before: "export const a = 1 /* c */ + 2;\n",
        after: "export const a = 1 + 2;\n",
        deleted: 1,
    },
    FixCase {
        name: "slashes-in-a-template-string-and-regex-stay",
        file: "case.ts",
        before: "export const a = `// keep`; // drop\nexport const b = /[//]/; // drop\n",
        after: "export const a = `// keep`;\nexport const b = /[//]/;\n",
        deleted: 2,
    },
    FixCase {
        name: "jsx-expression-comment",
        file: "case.tsx",
        before: "export const a = <div>{/* x */}</div>;\n",
        after: "export const a = <div>{}</div>;\n",
        deleted: 1,
    },
    FixCase {
        name: "comment-between-jsx-attributes",
        file: "case.tsx",
        before: "export const a = (\n  <div\n    // why\n    id=\"x\"\n  />\n);\n",
        after: "export const a = (\n  <div\n    id=\"x\"\n  />\n);\n",
        deleted: 1,
    },
    FixCase {
        name: "doc-comments-stay",
        file: "case.ts",
        before: "/** doc */\nexport const a = 1;\n/**\n * more\n */\nexport const b = 2;\n",
        after: "/** doc */\nexport const a = 1;\n/**\n * more\n */\nexport const b = 2;\n",
        deleted: 0,
    },
];

#[test]
fn rust_comments_are_deleted_and_nothing_else() {
    assert_fix_cases("fix-rust", RUST);
}

#[test]
fn typescript_comments_are_deleted_and_nothing_else() {
    assert_fix_cases("fix-ts", TYPESCRIPT);
}
