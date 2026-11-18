use super::support::{ScanCase, assert_scan_cases};

const CASES: &[ScanCase] = &[
    ScanCase {
        name: "use-list-names-each-module-at-its-top-level",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\n"),
            ("a.rs", "use crate::{b, c::Thing};\n"),
            ("b.rs", ""),
            ("c.rs", ""),
        ],
        edges: &["a.rs:1: a -> b", "a.rs:1: a -> c"],
    },
    ScanCase {
        name: "nested-use-list-counts-only-its-first-level",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            (
                "a.rs",
                "\
use crate::{
    b::{self, c, d::Deep},
    c as see,
};
",
            ),
            ("b.rs", "pub mod c {}\npub mod d {}\n"),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &["a.rs:2: a -> b", "a.rs:3: a -> c"],
    },
    ScanCase {
        name: "paths-in-expressions-types-and-patterns",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            (
                "a.rs",
                "\
fn f(thing: crate::b::Thing) -> u8 {
    match thing {
        crate::c::Kind::One => crate::d::value::<u8>(),
    }
}
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &["a.rs:1: a -> b", "a.rs:3: a -> c", "a.rs:3: a -> d"],
    },
    ScanCase {
        name: "the-crate-roots-own-items-are-main",
        files: &[
            ("main.rs", "mod a;\nmod b;\nfn init() {}\n"),
            ("a.rs", "fn f() {\n    crate::init();\n}\n"),
            ("b.rs", "use crate::*;\n"),
        ],
        edges: &["a.rs:2: a -> main", "b.rs:1: b -> main"],
    },
    ScanCase {
        name: "an-edge-is-reported-at-the-first-place-it-is-made",
        files: &[
            ("main.rs", "mod a;\nmod b;\n"),
            ("a.rs", "mod late;\nfn f() {}\n\nuse crate::b::Thing;\n"),
            ("a/late.rs", "use crate::b::Thing;\n"),
            ("b.rs", ""),
        ],
        edges: &["a.rs:4: a -> b"],
    },
    ScanCase {
        name: "paths-in-macro-arguments-count",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            (
                "a.rs",
                "\
fn f() {
    let _ = vec![crate::b::make()];
    let _ = format!(\"{}\", crate::c::NAME);
    items! {
        use crate::{d, b};
    }
}
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &["a.rs:2: a -> b", "a.rs:3: a -> c", "a.rs:5: a -> d"],
    },
    ScanCase {
        name: "dollar-crate-is-the-macros-own-crate",
        files: &[
            ("main.rs", "mod a;\nmod b;\n"),
            (
                "a.rs",
                "\
macro_rules! make {
    () => {
        $crate::b::make()
    };
}
",
            ),
            ("b.rs", ""),
        ],
        edges: &[],
    },
    ScanCase {
        name: "strings-and-comments-are-not-paths",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            (
                "a.rs",
                "\
/// See crate::b::Thing.
fn f() -> &'static str {
    \"crate::c::Thing\"
}
// crate::d::Thing
/* use crate::d; */
const RAW: &str = r#\"use crate::d;\"#;
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &[],
    },
    ScanCase {
        name: "a-module-reaching-itself-is-not-an-edge",
        files: &[
            ("main.rs", "mod a;\n"),
            (
                "a.rs",
                "\
use crate::a::Own;
pub(crate) fn f() {}
pub(super) fn g() {}
pub(in crate::a) fn h() {}
",
            ),
        ],
        edges: &[],
    },
];

#[test]
fn paths_rooted_at_crate_name_the_top_level_module_they_reach() {
    assert_scan_cases("paths", CASES);
}
