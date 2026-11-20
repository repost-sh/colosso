//! Which corners of a page's view are rounded: those it shares with its island, however the
//! frame that places it was drawn.

use gpui_kit::{Corners, Entity, TestAppContext};
use koyo_browser_ipc::Cursor;

use crate::browser::BrowserView;
use crate::budget::built_after;
use crate::settings::Layout;
use crate::test_support::{draw, open_page_view, shell_on};

fn rounded(view: &Entity<BrowserView>, cx: &mut TestAppContext) -> Corners<bool> {
    view.read_with(cx, |view, _| view.rounded)
}

/// In the Arc layout a page fills its pane's island to its foot, so its view rounds the island's
/// bottom corners. A frame that builds only the page, as the cursor over a link changes it,
/// draws the island around it as it was last frame, and the page keeps its corners.
#[gpui_kit::test]
fn a_page_built_alone_keeps_the_corners_it_shares_with_its_island(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("page-corners-alone", Layout::Arc, cx);
    let view = open_page_view(window, &shell, "https://example.com/", cx);
    let placed = rounded(&view, cx);
    assert!(placed.bottom_left && placed.bottom_right, "the island's foot is rounded: {placed:?}");

    let built = built_after(window, cx, |cx| {
        view.update(cx, |view, cx| view.cursor_changed(Cursor::Pointer, cx));
    });
    assert_eq!(built.shell, 0, "the cursor builds the page alone");
    assert_eq!(rounded(&view, cx), placed, "the page built alone");

    view.update(cx, |view, cx| view.progressed(40, cx));
    draw(window, cx);
    assert_eq!(rounded(&view, cx), placed, "the page loading");
}

/// A page full screen is on no island and rounds nothing; back in its pane, it rounds what it
/// shares with the pane's island again.
#[gpui_kit::test]
fn a_page_full_screen_is_square_and_rounded_again_back_in_its_pane(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("page-corners-full-screen", Layout::Arc, cx);
    let view = open_page_view(window, &shell, "https://example.com/", cx);
    let placed = rounded(&view, cx);
    assert!(placed.bottom_left && placed.bottom_right, "the island's foot is rounded: {placed:?}");

    view.update(cx, |view, cx| view.fullscreen_changed(true, cx));
    draw(window, cx);
    assert!(view.read_with(cx, |view, _| view.fills_screen()), "the page fills the screen");
    assert_eq!(rounded(&view, cx), Corners::default(), "full screen");

    view.update(cx, |view, cx| view.fullscreen_changed(false, cx));
    draw(window, cx);
    assert_eq!(rounded(&view, cx), placed, "back in its pane");
}
