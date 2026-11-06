//! The view in a window: a start page nobody holds that starts the host, the lease once the
//! person acts, and the questions it asks one at a time.

use gpui_kit::{AnyWindowHandle, TestAppContext, px, size};
use koyo_browser_ipc::Dialog;

use super::*;
use crate::browser::control::Mode;
use crate::browser::host::HostStatus;
// Drawing the window is when the view asks its questions.
use crate::test_support::{draw, in_window};

/// A start page in a window of its own. A test names no engine, so the host it starts
/// fails to.
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
fn a_start_page_shown_starts_the_host_before_any_address_is_asked_for(cx: &mut TestAppContext) {
    let (_, view) = start_page(cx);
    view.read_with(cx, |view, cx| {
        assert!(view.is_start_page());
        assert_ne!(view.status(cx), HostStatus::Idle);
    });
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

#[gpui_kit::test]
fn a_page_the_person_only_uses_goes_back_once_they_have_been_still_a_while(
    cx: &mut TestAppContext,
) {
    let (_, view) = start_page(cx);
    let still = |cx: &mut TestAppContext, secs: u64| {
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(secs));
        cx.run_until_parked();
    };
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    still(cx, 3);
    view.read_with(cx, |view, _| assert!(view.control.person_holds()));
    // Using it again holds it for as long again.
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    still(cx, 3);
    view.read_with(cx, |view, _| {
        assert!(view.control.person_holds(), "still at it");
    });
    still(cx, 2);
    view.read_with(cx, |view, _| {
        assert!(view.control.holder.is_none(), "gone back");
    });
    // Asked for, it stays theirs until they give it back.
    view.update(cx, |view, cx| view.person_acts(Mode::Explicit, cx));
    still(cx, 60);
    view.read_with(cx, |view, _| assert!(view.control.person_holds()));
    view.update(cx, BrowserView::give_back);
    view.read_with(cx, |view, _| assert!(view.control.holder.is_none()));
}
