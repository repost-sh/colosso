use crate::support::{CheckCase, assert_check_cases};

const FOUND: &[CheckCase] = &[
    CheckCase {
        name: "line-comment",
        file: "case.rs",
        source: "// hello\nfn main() {}\n",
        findings: &["case.rs:1:1: comment: // hello"],
    },
    CheckCase {
        name: "indented-line-comment",
        file: "case.rs",
        source: "fn main() {\n    // inside\n}\n",
        findings: &["case.rs:2:5: comment: // inside"],
    },
    CheckCase {
        name: "trailing-line-comment",
        file: "case.rs",
        source: "fn main() {} // after code\n",
        findings: &["case.rs:1:14: comment: // after code"],
    },
    CheckCase {
        name: "block-comment",
        file: "case.rs",
        source: "/* hello */\nfn main() {}\n",
        findings: &["case.rs:1:1: comment: /* hello */"],
    },
    CheckCase {
        name: "block-comment-between-tokens",
        file: "case.rs",
        source: "fn main(/* nothing */) {}\n",
        findings: &["case.rs:1:9: comment: /* nothing */"],
    },
    CheckCase {
        name: "nested-block-comment",
        file: "case.rs",
        source: "/* outer /* inner */ still outer */\nfn main() {}\n",
        findings: &["case.rs:1:1: comment: /* outer /* inner */ still outer */"],
    },
    CheckCase {
        name: "multi-line-block-comment-shows-its-first-line",
        file: "case.rs",
        source: "fn main() {}\n  /* first\n     second */\n",
        findings: &["case.rs:2:3: comment: /* first"],
    },
    CheckCase {
        name: "four-slashes-is-plain",
        file: "case.rs",
        source: "//// banner\nfn main() {}\n",
        findings: &["case.rs:1:1: comment: //// banner"],
    },
    CheckCase {
        name: "empty-block-comment-is-plain",
        file: "case.rs",
        source: "fn main() {} /**/\n",
        findings: &["case.rs:1:14: comment: /**/"],
    },
    CheckCase {
        name: "column-counts-characters-not-bytes",
        file: "case.rs",
        source: "fn main() { let s = \"é\"; // c\n}\n",
        findings: &["case.rs:1:26: comment: // c"],
    },
    CheckCase {
        name: "several-in-order",
        file: "case.rs",
        source: "// one\nfn main() {\n    let x = 1; // two\n    /* three */\n}\n",
        findings: &[
            "case.rs:1:1: comment: // one",
            "case.rs:3:16: comment: // two",
            "case.rs:4:5: comment: /* three */",
        ],
    },
    CheckCase {
        name: "crlf-lines-keep-their-columns",
        file: "case.rs",
        source: "fn main() {}\r\n  // c\r\n",
        findings: &["case.rs:2:3: comment: // c"],
    },
    CheckCase {
        name: "a-comment-in-a-macro-body-is-found",
        file: "case.rs",
        source: "m! {\n    // inside a macro\n    fn f() {}\n}\n",
        findings: &["case.rs:2:5: comment: // inside a macro"],
    },
];

const BESIDE_DOC_MARKERS: &[CheckCase] = &[
    CheckCase {
        name: "plain-comment-after-a-doc-comment",
        file: "case.rs",
        source: "/// doc\n// plain\nfn main() {}\n",
        findings: &["case.rs:2:1: comment: // plain"],
    },
    CheckCase {
        name: "plain-comment-straight-after-a-doc-block",
        file: "case.rs",
        source: "/** doc *///plain\nfn main() {}\n",
        findings: &["case.rs:1:11: comment: //plain"],
    },
    CheckCase {
        name: "doc-marker-inside-a-string-then-a-plain-comment",
        file: "case.rs",
        source: "fn main() { let s = \"///\"; // plain\n}\n",
        findings: &["case.rs:1:28: comment: // plain"],
    },
    CheckCase {
        name: "inner-doc-marker-inside-a-string-then-a-plain-comment",
        file: "case.rs",
        source: "fn main() { let s = \"//!\"; /* plain */\n}\n",
        findings: &["case.rs:1:28: comment: /* plain */"],
    },
    CheckCase {
        name: "block-doc-marker-inside-a-string-then-a-plain-comment",
        file: "case.rs",
        source: "fn main() { let s = \"/**\"; // plain\n}\n",
        findings: &["case.rs:1:28: comment: // plain"],
    },
    CheckCase {
        name: "plain-comment-after-the-url-in-a-doc-comment",
        file: "case.rs",
        source: "/// see https://example.com\n// plain\nfn main() {}\n",
        findings: &["case.rs:2:1: comment: // plain"],
    },
    CheckCase {
        name: "block-comment-with-three-stars-is-plain",
        file: "case.rs",
        source: "/*** banner ***/\nfn main() {}\n",
        findings: &["case.rs:1:1: comment: /*** banner ***/"],
    },
    CheckCase {
        name: "block-comment-after-a-doc-block",
        file: "case.rs",
        source: "/** doc */ /* plain */\nfn main() {}\n",
        findings: &["case.rs:1:12: comment: /* plain */"],
    },
];

const CLEAN: &[CheckCase] = &[
    CheckCase {
        name: "no-comments",
        file: "case.rs",
        source: "fn main() {}\n",
        findings: &[],
    },
    CheckCase {
        name: "empty-file",
        file: "case.rs",
        source: "",
        findings: &[],
    },
    CheckCase {
        name: "doc-comments",
        file: "case.rs",
        source: "//! inner line\n/*! inner block */\n/// outer line\n/** outer block */\nfn main() {}\n",
        findings: &[],
    },
    CheckCase {
        name: "empty-doc-comments",
        file: "case.rs",
        source: "///\n//!\nfn main() {}\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-string",
        file: "case.rs",
        source: "fn main() { let _ = \"https://x\"; }\n",
        findings: &[],
    },
    CheckCase {
        name: "block-markers-in-a-string",
        file: "case.rs",
        source: "fn main() { let _ = \"/* x */\"; }\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-raw-string",
        file: "case.rs",
        source: "fn main() { let _ = r#\"// no\"#; }\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-multi-line-string",
        file: "case.rs",
        source: "fn main() {\n    let _ = \"a\n// not a comment\nb\";\n}\n",
        findings: &[],
    },
    CheckCase {
        name: "char-and-lifetimes",
        file: "case.rs",
        source: "fn f<'a>(text: &'a str) -> (char, &'static str) { ('/', text) }\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-byte-string",
        file: "case.rs",
        source: "fn main() { let _ = b\"//\"; }\n",
        findings: &[],
    },
];

#[test]
fn rust_comments_are_found_with_line_and_column() {
    assert_check_cases("rust-found", FOUND);
}

#[test]
fn a_plain_comment_is_found_beside_text_that_looks_like_a_doc_marker() {
    assert_check_cases("rust-beside-doc", BESIDE_DOC_MARKERS);
}

#[test]
fn rust_code_and_doc_comments_are_clean() {
    assert_check_cases("rust-clean", CLEAN);
}
