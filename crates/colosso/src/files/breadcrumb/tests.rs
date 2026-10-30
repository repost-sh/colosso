//! The breadcrumb over the file shown names its folders and the file; a crumb clicked is
//! picked in the tree, its folders opened, and the tree takes the keyboard.

use std::path::Path;

use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

use super::{Crumb, crumbs};
use crate::files::At;
use crate::files::tests::{checkout, editing, open, with};

#[test]
fn the_crumbs_are_the_folders_as_the_tree_names_them_then_the_file() {
    let crumb = |label: &str, path: Option<&str>| Crumb {
        label: label.to_owned().into(),
        path: path.map(Into::into),
    };
    assert_eq!(
        crumbs(Path::new("/r/src/nested/deep.rs"), Some("src/nested/deep.rs")),
        [
            crumb("src", Some("src")),
            crumb("nested", Some("src/nested")),
            crumb("deep.rs", Some("src/nested/deep.rs")),
        ]
    );
    assert_eq!(crumbs(Path::new("/r/README.md"), Some("README.md")), [crumb("README.md", Some("README.md"))]);
    // Outside the checkout: the whole folder, which the tree hasn't got, then the file.
    assert_eq!(crumbs(Path::new("/opt/x/lib.rs"), None), [crumb("/opt/x", None), crumb("lib.rs", None)]);
}

#[gpui_kit::test]
fn a_crumb_clicked_is_picked_in_the_tree_with_its_folders_opened(cx: &mut TestAppContext) {
    let root = checkout("files-breadcrumb");
    let (window, panel) = open(&root, cx);
    editing(window, &panel, "src/nested/deep.rs", At::Bytes(0..0), cx);
    // `src` closed again, nothing under it is listed.
    with(window, &panel, cx, |panel, _, cx| {
        let src = panel.rows.position("src").expect("src listed");
        panel.toggle(src, cx);
    });
    let listed = |path: &str, cx: &mut VisualTestContext| cx.read(|cx| panel.read(cx).rows.position(path).is_some());
    let selected = |cx: &mut VisualTestContext| {
        cx.read(|cx| panel.read(cx).selected.as_deref().map(str::to_owned))
    };
    let click = |selector: &'static str, cx: &mut VisualTestContext| {
        let crumb = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(crumb.center(), Modifiers::none());
    };
    let cx = &mut VisualTestContext::from_window(window, cx);
    cx.run_until_parked();
    assert!(!listed("src/nested", cx));
    // "nested" clicked: src opens, nested is picked and the tree has the keyboard.
    click("crumb-1", cx);
    assert_eq!(selected(cx).as_deref(), Some("src/nested"));
    assert!(listed("src/nested", cx));
    assert!(cx.update(|window, cx| panel.read(cx).focus_handle().is_focused(window)));
    click("crumb-0", cx);
    assert_eq!(selected(cx).as_deref(), Some("src"));
    // The file's name picks the file.
    click("crumb-2", cx);
    assert_eq!(selected(cx).as_deref(), Some("src/nested/deep.rs"));
    let _ = std::fs::remove_dir_all(&root);
}
