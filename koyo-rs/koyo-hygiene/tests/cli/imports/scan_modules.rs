use super::support::{ScanCase, assert_scan_cases};

const SUPER: &[ScanCase] = &[
    ScanCase {
        name: "super-from-a-top-level-module-reaches-the-root",
        files: &[
            ("main.rs", "mod a;\nmod b;\nfn open_main_window() {}\n"),
            (
                "a.rs",
                "use super::b::Thing;\nfn f() {\n    super::open_main_window();\n}\n",
            ),
            ("b.rs", ""),
        ],
        edges: &["a.rs:1: a -> b", "a.rs:3: a -> main"],
    },
    ScanCase {
        name: "super-chains-at-each-depth",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            ("a.rs", "mod inner;\n"),
            (
                "a/inner.rs",
                "mod deeper;\nuse super::super::b::Thing;\nuse super::sibling;\n",
            ),
            (
                "a/inner/deeper.rs",
                "\
use super::super::super::c::Thing;
use super::super::own;
use self::super::super::super::d::Thing;
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &[
            "a/inner.rs:2: a -> b",
            "a/inner/deeper.rs:1: a -> c",
            "a/inner/deeper.rs:3: a -> d",
        ],
    },
    ScanCase {
        name: "a-use-list-element-can-climb-further",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\nmod d;\n"),
            ("a.rs", "mod inner;\n"),
            (
                "a/inner.rs",
                "\
use super::{
    super::b::Thing,
    sibling,
    super::{c, d::Other},
};
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &[
            "a/inner.rs:2: a -> b",
            "a/inner.rs:4: a -> c",
            "a/inner.rs:4: a -> d",
        ],
    },
    ScanCase {
        name: "an-inline-module-adds-a-level",
        files: &[
            ("main.rs", "mod a;\nmod b;\nmod c;\n"),
            (
                "a.rs",
                "\
mod imp {
    use super::super::b::Thing;
    use super::own;
    mod deeper {
        use super::super::super::c::Thing;
        use super::super::own;
    }
}
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
        ],
        edges: &["a.rs:2: a -> b", "a.rs:5: a -> c"],
    },
    ScanCase {
        name: "an-inline-module-of-the-root-is-a-top-level-module",
        files: &[
            (
                "main.rs",
                "\
mod a;
mod b;
mod inline {
    use super::b::Thing;
    fn f() {
        crate::a::g();
    }
}
",
            ),
            ("a.rs", "use crate::inline::f;\n"),
            ("b.rs", ""),
        ],
        edges: &[
            "a.rs:1: a -> inline",
            "main.rs:4: inline -> b",
            "main.rs:6: inline -> a",
        ],
    },
    ScanCase {
        name: "a-glob-of-the-root-is-main",
        files: &[
            ("main.rs", "mod a;\nmod b;\n"),
            ("a.rs", "use super::*;\n"),
            ("b.rs", "mod inner {\n    use super::*;\n}\n"),
        ],
        edges: &["a.rs:1: a -> main"],
    },
];

const FILES: &[ScanCase] = &[
    ScanCase {
        name: "a-path-attribute-names-the-file",
        files: &[
            (
                "main.rs",
                "mod a;\nmod b;\nmod c;\n#[path = \"elsewhere/d_impl.rs\"]\nmod d;\n",
            ),
            (
                "a/mod.rs",
                "\
#[cfg(target_os = \"macos\")]
#[path = \"native_on.rs\"]
mod native;
#[cfg(not(target_os = \"macos\"))]
#[path = \"native_off.rs\"]
mod native;
",
            ),
            ("a/native_on.rs", "use crate::b::On;\n"),
            ("a/native_off.rs", "use crate::c::Off;\n"),
            ("b.rs", ""),
            ("c.rs", ""),
            ("elsewhere/d_impl.rs", "mod sub;\n"),
            ("elsewhere/sub.rs", "use crate::a::Thing;\n"),
        ],
        edges: &["a/native_on.rs:1: a -> b", "elsewhere/sub.rs:1: d -> a"],
    },
    ScanCase {
        name: "a-path-attribute-in-an-inline-module",
        files: &[
            ("main.rs", "mod a;\nmod b;\n"),
            (
                "a.rs",
                "mod imp {\n    #[path = \"chosen.rs\"]\n    mod picked;\n    mod plain;\n}\n",
            ),
            ("a/imp/chosen.rs", "use crate::b::Thing;\n"),
            ("a/imp/plain.rs", "use super::super::own;\n"),
            ("b.rs", ""),
        ],
        edges: &["a/imp/chosen.rs:1: a -> b"],
    },
    ScanCase {
        name: "a-module-file-beside-a-file-that-is-not-mod-rs",
        files: &[
            ("main.rs", "mod a;\nmod b;\n"),
            ("a.rs", "mod part;\n"),
            ("a/part.rs", "mod deeper;\n"),
            ("a/part/deeper/mod.rs", "use crate::b::Thing;\n"),
            ("b.rs", ""),
        ],
        edges: &["a/part/deeper/mod.rs:1: a -> b"],
    },
];

#[test]
fn super_chains_resolve_against_the_module_and_its_inline_modules() {
    assert_scan_cases("super", SUPER);
}

#[test]
fn mod_declarations_find_their_files_as_rustc_does() {
    assert_scan_cases("files", FILES);
}
