//! Where the harness finds the parts a check on screen asks about (`crate::marks`).

use gpui_kit::TestAppContext;

use crate::marks::find;
use crate::model::PaneKind;
use crate::test_support::{draw, open_page, repo, select, shell_with};

/// The harness finds a page and a chat's composer by name once they're drawn.
#[gpui_kit::test]
fn a_page_and_the_composer_are_found_where_they_were_drawn(cx: &mut TestAppContext) {
    let root = repo("marks-drawn", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let pane = open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);
    let page_id = shell.read_with(cx, |shell, cx| {
        shell.workspace.pane(pane).unwrap().view().unwrap().read(cx).page_id()
    });
    let (drawn_in, bounds) = find(&format!("page-{page_id}")).unwrap();
    assert_eq!(drawn_in, window.window_id().as_u64());
    assert!(bounds.size.width.as_f32() > 0., "{bounds:?}");
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    draw(window, cx);
    assert!(find("chat-composer").is_some());
}
