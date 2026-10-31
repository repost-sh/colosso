//! The view in a window: a start page nobody holds, the lease once the person acts, and the
//! questions it asks one at a time.

use colosso_browser_ipc::Dialog;
use gpui_kit::{AnyWindowHandle, TestAppContext, px, size};

use super::*;
// Drawing the window is when the view asks its questions.
use crate::test_support::{draw, in_window};

/// A start page in a window of its own; no host starts for it.
fn start_page(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<BrowserView>) {
    cx.update(crate::init);
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx))
    });
    draw(window, cx);
    (window, view)
}

#[gpui_kit::test]
fn a_new_tab_is_a_start_page_nobody_holds_until_the_person_acts(cx: &mut TestAppContext) {
    let (_, view) = start_page(cx);
    view.read_with(cx, |view, _| {
        assert!(view.is_start_page());
        assert_eq!(view.title(), "New tab");
        assert!(!view.is_inspector());
        assert!(view.control.holder.is_none());
    });
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    view.read_with(cx, |view, _| assert!(view.control.person_holds()));
    view.update(cx, BrowserView::give_back);
    view.read_with(cx, |view, _| assert!(view.control.holder.is_none()));
}

#[gpui_kit::test]
fn questions_are_asked_one_at_a_time_and_a_withdrawn_one_makes_way(cx: &mut TestAppContext) {
    let (window, view) = start_page(cx);
    view.update(cx, |view, cx| {
        let alert = Dialog::Alert {
            message: "Hi".into(),
        };
        view.dialog_opened(1, "https://example.com".into(), alert, cx);
        let confirm = Dialog::Confirm {
            message: "Sure?".into(),
        };
        view.dialog_opened(2, "https://example.com".into(), confirm, cx);
    });
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.asked, Some(1), "the first is asked");
        assert_eq!(view.dialogs.len(), 2, "the second waits");
    });
    view.update(cx, |view, cx| view.dialog_closed(1, cx));
    draw(window, cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.asked, Some(2));
        assert_eq!(view.dialogs.len(), 1);
    });
}
