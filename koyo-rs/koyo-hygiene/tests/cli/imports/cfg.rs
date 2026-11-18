use super::support::{ScanCase, assert_scan_cases};

const MODULES: &str = "mod a;\nmod b;\nmod c;\nmod d;\n";

const CASES: &[ScanCase] = &[
    ScanCase {
        name: "test-items-are-left-out",
        files: &[
            ("main.rs", MODULES),
            (
                "a.rs",
                "\
#[cfg(test)]
use crate::b::Thing;
#[cfg(all(test, target_os = \"macos\"))]
fn helper() {
    crate::c::make();
}
#[cfg(test)]
mod tests {
    use crate::d::Thing;
}
#[test]
fn checks() {
    crate::b::make();
}
#[gpui::test]
fn checks_the_app() {
    crate::c::make();
}
fn counted() {
    match 1 {
        #[cfg(test)]
        0 => crate::d::make(),
        _ => {}
    }
    #[cfg(test)]
    crate::b::make();
}
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &[],
    },
    ScanCase {
        name: "features-are-off",
        files: &[
            (
                "main.rs",
                "\
mod a;
mod b;
mod c;
mod d;
#[cfg(feature = \"visual-check\")]
mod visual;
#[cfg(any(test, feature = \"visual-check\"))]
mod test_support;
",
            ),
            (
                "a.rs",
                "\
#[cfg(feature = \"visual-check\")]
use crate::b::Thing;
#[cfg(any(test, feature = \"visual-check\"))]
use crate::c::Thing;
#[cfg(all(target_os = \"macos\", feature = \"visual-check\"))]
use crate::d::Thing;
#[cfg(not(feature = \"visual-check\"))]
use crate::b::Other;
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
            ("test_support.rs", "use crate::a::Thing;\n"),
        ],
        edges: &["a.rs:8: a -> b"],
    },
    ScanCase {
        name: "every-other-option-is-on",
        files: &[
            ("main.rs", MODULES),
            (
                "a.rs",
                "\
#[cfg(target_os = \"macos\")]
use crate::b::Thing;
#[cfg(not(target_os = \"macos\"))]
use crate::c::Thing;
#[cfg(debug_assertions)]
use crate::d::Thing;
#[cfg(all(target_os = \"macos\", debug_assertions, not(test)))]
use crate::c::Other;
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &["a.rs:2: a -> b", "a.rs:6: a -> d", "a.rs:8: a -> c"],
    },
    ScanCase {
        name: "all-any-and-not-combine-as-rustc-does",
        files: &[
            ("main.rs", MODULES),
            (
                "a.rs",
                "\
#[cfg(any(test, target_os = \"macos\"))]
use crate::b::Thing;
#[cfg(any())]
use crate::c::Thing;
#[cfg(all())]
use crate::d::Thing;
#[cfg(not(any(test, feature = \"slow\")))]
use crate::c::Other;
",
            ),
            ("b.rs", ""),
            ("c.rs", ""),
            ("d.rs", ""),
        ],
        edges: &["a.rs:2: a -> b", "a.rs:6: a -> d", "a.rs:8: a -> c"],
    },
    ScanCase {
        name: "a-mod-declaration-left-out-leaves-out-its-files",
        files: &[
            (
                "main.rs",
                "\
mod a;
mod b;
#[cfg(test)]
mod budget;
#[cfg(not(target_os = \"macos\"))]
mod elsewhere;
",
            ),
            (
                "a.rs",
                "\
#[cfg(test)]
mod tests;
#[cfg(feature = \"slow\")]
mod not_written;
mod gated;
mod featured;
",
            ),
            ("a/tests.rs", "use crate::b::Thing;\n"),
            ("a/gated.rs", "#![cfg(test)]\nuse crate::b::Thing;\n"),
            (
                "a/featured.rs",
                "#![cfg(feature = \"visual-check\")]\nuse crate::b::Thing;\n",
            ),
            ("b.rs", ""),
            ("budget.rs", "use crate::a::Thing;\n"),
            ("elsewhere.rs", "use crate::a::Thing;\n"),
        ],
        edges: &[],
    },
];

#[test]
fn only_code_the_shipped_build_compiles_counts() {
    assert_scan_cases("cfg", CASES);
}
