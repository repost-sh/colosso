use super::support::{ScanCase, assert_scan_cases};

const CASES: &[ScanCase] = &[
    ScanCase {
        name: "a-bare-path-in-the-crate-root-names-the-module-it-starts-with",
        files: &[
            (
                "main.rs",
                "\
mod a;
mod b;
mod c;
mod d;
use gpui_kit::{App, c::Thing};
fn init() {
    a::init();
    koyo_term::init();
    let _ = Vec::<b::Thing>::new();
    #[cfg(feature = \"visual-check\")]
    c::visual();
    #[cfg(target_os = \"macos\")]
    d::native();
    self::a::again();
}
",
            ),
            ("a.rs", ""),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &[
            "main.rs:7: main -> a",
            "main.rs:9: main -> b",
            "main.rs:13: main -> d",
        ],
    },
    ScanCase {
        name: "a-bare-path-anywhere-else-starts-in-its-own-module",
        files: &[
            (
                "main.rs",
                "\
mod a;
mod b;
mod c;
mod inline {
    fn f() {
        c::g();
    }
}
",
            ),
            (
                "a.rs",
                "mod b;\nfn f() {\n    b::g();\n    self::c::h();\n}\n",
            ),
            ("a/b.rs", ""),
            ("b.rs", ""),
            ("c.rs", ""),
        ],
        edges: &[],
    },
];

#[test]
fn the_crate_roots_own_code_reaches_modules_by_bare_paths() {
    assert_scan_cases("root", CASES);
}
