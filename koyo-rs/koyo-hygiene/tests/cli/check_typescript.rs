use crate::support::{CheckCase, assert_check_cases};

const FOUND: &[CheckCase] = &[
    CheckCase {
        name: "line-comment",
        file: "case.ts",
        source: "// hello\nexport const a = 1;\n",
        findings: &["case.ts:1:1: comment: // hello"],
    },
    CheckCase {
        name: "trailing-line-comment",
        file: "case.ts",
        source: "export const a = 1; // after\n",
        findings: &["case.ts:1:21: comment: // after"],
    },
    CheckCase {
        name: "block-comment",
        file: "case.ts",
        source: "/* hello */\nexport const a = 1;\n",
        findings: &["case.ts:1:1: comment: /* hello */"],
    },
    CheckCase {
        name: "empty-block-comment-is-plain",
        file: "case.ts",
        source: "export const a = /**/ 1;\n",
        findings: &["case.ts:1:18: comment: /**/"],
    },
    CheckCase {
        name: "multi-line-block-shows-its-first-line",
        file: "case.ts",
        source: "export const a = 1;\n  /* first\n     second */\n",
        findings: &["case.ts:2:3: comment: /* first"],
    },
    CheckCase {
        name: "comment-inside-a-type",
        file: "case.ts",
        source: "export type T = {\n  // why\n  a: number;\n};\n",
        findings: &["case.ts:2:3: comment: // why"],
    },
    CheckCase {
        name: "jsx-expression-comment",
        file: "case.tsx",
        source: "export const a = <div>{/* x */}</div>;\n",
        findings: &["case.tsx:1:24: comment: /* x */"],
    },
    CheckCase {
        name: "tsx-line-comment-between-attributes",
        file: "case.tsx",
        source: "export const a = (\n  <div\n    // why\n    id=\"x\"\n  />\n);\n",
        findings: &["case.tsx:3:5: comment: // why"],
    },
];

const BESIDE_DOC_MARKERS: &[CheckCase] = &[
    CheckCase {
        name: "line-comment-after-a-doc-comment",
        file: "case.ts",
        source: "/** doc */ // plain\nexport const a = 1;\n",
        findings: &["case.ts:1:12: comment: // plain"],
    },
    CheckCase {
        name: "triple-slash-directive-is-plain",
        file: "case.ts",
        source: "/// <reference types=\"node\" />\nexport const a = 1;\n",
        findings: &["case.ts:1:1: comment: /// <reference types=\"node\" />"],
    },
    CheckCase {
        name: "bang-block-comment-is-plain",
        file: "case.ts",
        source: "/*! license */\nexport const a = 1;\n",
        findings: &["case.ts:1:1: comment: /*! license */"],
    },
    CheckCase {
        name: "doc-marker-inside-a-string-then-a-plain-comment",
        file: "case.ts",
        source: "export const a = \"/**\"; // plain\n",
        findings: &["case.ts:1:25: comment: // plain"],
    },
    CheckCase {
        name: "plain-block-after-a-doc-block",
        file: "case.ts",
        source: "/** doc */ /* plain */\nexport const a = 1;\n",
        findings: &["case.ts:1:12: comment: /* plain */"],
    },
];

const CLEAN: &[CheckCase] = &[
    CheckCase {
        name: "doc-comment",
        file: "case.ts",
        source: "/** hello */\nexport const a = 1;\n",
        findings: &[],
    },
    CheckCase {
        name: "multi-line-doc-comment",
        file: "case.ts",
        source: "/**\n * hello\n */\nexport const a = 1;\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-template-string",
        file: "case.ts",
        source: "export const a = `https://x/${b}// not a comment`;\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-a-regex-class",
        file: "case.ts",
        source: "export const a = /[//]/;\n",
        findings: &[],
    },
    CheckCase {
        name: "escaped-slashes-in-a-regex",
        file: "case.ts",
        source: "export const a = /https?:\\/\\/x/u;\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-strings",
        file: "case.ts",
        source: "export const a = 'https://x', b = \"// y\", c = \"/* z */\";\n",
        findings: &[],
    },
    CheckCase {
        name: "slashes-in-jsx-text",
        file: "case.tsx",
        source: "export const a = <div>// not a comment</div>;\n",
        findings: &[],
    },
    CheckCase {
        name: "no-comments",
        file: "case.tsx",
        source: "export const a = <div id=\"x\">{1}</div>;\n",
        findings: &[],
    },
];

#[test]
fn typescript_comments_are_found_with_line_and_column() {
    assert_check_cases("ts-found", FOUND);
}

#[test]
fn a_plain_comment_is_found_beside_text_that_looks_like_a_doc_marker() {
    assert_check_cases("ts-beside-doc", BESIDE_DOC_MARKERS);
}

#[test]
fn typescript_code_and_doc_comments_are_clean() {
    assert_check_cases("ts-clean", CLEAN);
}
