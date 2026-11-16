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

#[gpui_kit::test]
fn the_agents_pointer_travels_only_where_the_person_can_see_it(cx: &mut TestAppContext) {
    let (window, drawn) = start_page(cx);
    // A page the window holds but doesn't draw, as a tab out of sight.
    let unseen = window
        .update(cx, |_, window, cx| {
            cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx))
        })
        .unwrap();
    let travel = |view: &Entity<BrowserView>, cx: &mut TestAppContext| {
        view.update(cx, |view, cx| view.point_to(300., 200., cx))
    };
    assert!(
        travel(&drawn, cx) > std::time::Duration::ZERO,
        "in sight, the person sees it travel to where it clicks"
    );
    assert_eq!(
        travel(&unseen, cx),
        std::time::Duration::ZERO,
        "out of sight, nobody waits for it"
    );
}

#[gpui_kit::test]
fn the_devtools_own_controls_move_and_close_them_and_turn_responsive_mode_on_and_off(
    cx: &mut TestAppContext,
) {
    use crate::browser::Dock;
    use koyo_browser_ipc::{DevToolsAsk, DevToolsDock};
    let (window, view) = start_page(cx);
    let device = |on| DevToolsAsk::DeviceMode { on };
    let responsive =
        |cx: &mut TestAppContext| view.read_with(cx, |view, _| view.responsive.mode.is_some());
    // Without open DevTools, nothing they'd ask changes the page.
    view.update(cx, |view, cx| view.devtools_asked(device(true), cx));
    assert!(!responsive(cx));

    view.update(cx, |view, cx| view.inspect(None, cx));
    draw(window, cx);
    let side = |cx: &mut TestAppContext| view.read_with(cx, BrowserView::devtools_side);
    assert_eq!(
        side(cx),
        DevToolsDock::Bottom,
        "they open where the next ones open"
    );
    for (asked, dock) in [
        (DevToolsDock::Left, Dock::Left),
        (DevToolsDock::Left, Dock::Left),
        (DevToolsDock::Right, Dock::Right),
        (DevToolsDock::Undocked, Dock::Detached),
        (DevToolsDock::Bottom, Dock::Bottom),
    ] {
        let ask = DevToolsAsk::Dock { side: asked };
        view.update(cx, |view, cx| view.devtools_asked(ask, cx));
        view.read_with(cx, |view, _| assert_eq!(view.devtools_dock(), Some(dock)));
        assert_eq!(side(cx), asked);
        assert_eq!(
            cx.update(|cx| crate::settings::get(cx).devtools.dock),
            dock,
            "kept"
        );
    }

    // The device toolbar toggle sets responsive mode, said twice or not.
    for (on, then) in [(true, true), (true, true), (false, false), (false, false)] {
        view.update(cx, |view, cx| view.devtools_asked(device(on), cx));
        assert_eq!(responsive(cx), then);
    }

    view.update(cx, |view, cx| view.devtools_asked(DevToolsAsk::Close, cx));
    view.read_with(cx, |view, _| assert!(view.devtools().is_none(), "closed"));

    // Toggled with the keys in the tools themselves, they close as from their page.
    view.update(cx, |view, cx| view.inspect(None, cx));
    draw(window, cx);
    let tools = view.read_with(cx, |view, _| view.devtools().cloned().expect("open again"));
    tools.update(cx, |tools, cx| tools.inspect(None, cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.devtools().is_none(), "closed from inside"));
}

/// A page's pane id taken by another view, as a page opened again there takes it: the first
/// view going lets go of nothing but its own.
#[gpui_kit::test]
fn a_view_going_leaves_its_pane_to_the_view_that_took_it_since(cx: &mut TestAppContext) {
    let (window, first) = start_page(cx);
    let (_, second) = start_page(cx);
    let pane = first.read_with(cx, |view, _| view.pane);
    let browsers = cx.update(Browsers::global);
    browsers.update(cx, |browsers, _| browsers.attach(pane, second.downgrade()));
    // The first view goes with its window.
    let gone = first.downgrade();
    drop(first);
    (cx.update_window(window, |_, window, _| window.remove_window())).unwrap();
    cx.run_until_parked();
    assert!(gone.upgrade().is_none(), "the first view is released");
    let kept = browsers.read_with(cx, |browsers, _| browsers.view(pane));
    assert_eq!(kept, Some(second.clone()), "the second view keeps the pane");
    browsers.update(cx, |browsers, _| browsers.detach(pane, second.entity_id()));
    assert_eq!(browsers.read_with(cx, |browsers, _| browsers.view(pane)), None);
}

/// ⌥⌘I with the keyboard in the page's `DevTools` hides them and gives the keyboard back to
/// the page, as the person asked them gone.
#[gpui_kit::test]
fn command_option_i_out_of_the_devtools_gives_the_keyboard_back_to_the_page(
    cx: &mut TestAppContext,
) {
    use crate::keyboard::{self, Holder};
    let (window, view) = start_page(cx);
    (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    cx.run_until_parked();
    view.update(cx, |view, cx| view.inspect(None, cx));
    draw(window, cx);
    let tools = view.read_with(cx, |view, _| view.devtools().cloned()).unwrap();
    let ids = (view.read_with(cx, |view, _| view.pane), tools.read_with(cx, |tools, _| tools.pane));
    (cx.update_window(window, |_, window, cx| {
        let focus = tools.read(cx).focus.clone();
        window.focus(&focus, cx);
    }))
    .unwrap();
    draw(window, cx);
    assert_eq!(keyboard::holder(window), Some(Holder::Page(ids.1)));
    (cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.toggle_devtools(window, cx));
    }))
    .unwrap();
    assert_eq!(keyboard::holder(window), Some(Holder::Page(ids.0)), "at once");
    draw(window, cx);
    view.read_with(cx, |view, _| assert!(view.devtools().is_none(), "hidden"));
    let in_page = cx.update_window(window, |_, window, cx| view.read(cx).focus.is_focused(window));
    assert!(in_page.unwrap());
}
