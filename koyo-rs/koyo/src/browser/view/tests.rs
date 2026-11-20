//! The view in a window: a start page nobody holds that starts the host, the lease once the
//! person acts, and the questions it asks one at a time.

use gpui_kit::{AnyWindowHandle, TestAppContext, px, size};
use koyo_browser_ipc::Dialog;

use super::*;
use crate::browser::control::{Leased as _, Mode};
use crate::browser::host::HostStatus;
use crate::test_support::{draw, in_window};

mod keys;

/// A start page in a window of its own. A test names no engine, so the host it starts
/// fails to.
fn start_page(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<BrowserView>) {
    cx.update(crate::test_support::init);
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
        assert!(view.lease.control.holder.is_none());
    });
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    view.read_with(cx, |view, _| assert!(view.lease.control.person_holds()));
    view.update(cx, BrowserView::give_back);
    view.read_with(cx, |view, _| assert!(view.lease.control.holder.is_none()));
}

/// The engine starts once while Koyo runs, so a page whose engine didn't start says what
/// starts it again: Koyo quit and opened again.
#[gpui_kit::test]
fn a_page_whose_engine_did_not_start_says_what_starts_it_again(cx: &mut TestAppContext) {
    let (_, view) = start_page(cx);
    let status = view.read_with(cx, BrowserView::status);
    let HostStatus::Failed(message) = status else {
        panic!("a test's engine never starts: {status:?}");
    };
    assert!(message.ends_with(crate::browser::STARTS_AGAIN), "{message}");
}

#[gpui_kit::test]
fn a_start_page_shown_starts_the_host_before_any_address_is_asked_for(cx: &mut TestAppContext) {
    let (_, view) = start_page(cx);
    view.read_with(cx, |view, cx| {
        assert!(view.is_start_page());
        assert_ne!(view.status(cx), HostStatus::Idle);
    });
}

/// A page whose renderer went away says so, and its Reload has the host reload the page it
/// still has, as Chrome's does: Chromium gives it a new renderer. The host has no use for an
/// Open of a page it has.
#[gpui_kit::test]
fn reload_after_a_crash_reloads_the_page_the_host_still_has(cx: &mut TestAppContext) {
    use crate::browser::host::fake::FakeHost;
    use koyo_browser_ipc::{AppRequest, Go, HostEvent};
    cx.update(crate::test_support::init);
    let host = FakeHost::serve("crash-reload", cx);
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        let url = "https://example.com/".to_owned();
        cx.new(|cx| BrowserView::new(url, Path::new("/work/app"), window, cx))
    });
    draw(window, cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    let opened = host.asked(cx);
    let opens = |asked: &AppRequest| {
        matches!(asked, AppRequest::Open { pane: of, .. } if *of == pane)
    };
    assert!(opened.iter().any(opens), "the page opens: {opened:?}");
    let reason = "This page crashed.";
    host.says(HostEvent::Gone { pane, reason: reason.into() }, cx);
    view.read_with(cx, |view, cx| {
        assert!(matches!(&view.notice, Some(Notice::Gone(said)) if said == reason));
        assert!(view.controls(cx).reload, "Reload is offered");
    });
    view.update(cx, |view, cx| view.go(Go::Reload, cx));
    let loads: Vec<AppRequest> = (host.asked(cx).into_iter())
        .filter(|asked| matches!(asked, AppRequest::Open { .. } | AppRequest::Go { .. }))
        .collect();
    assert_eq!(loads, [AppRequest::Go { pane, to: Go::Reload }]);
    view.read_with(cx, |view, _| assert!(view.notice.is_none(), "the notice goes"));

    host.says(HostEvent::Gone { pane, reason: reason.into() }, cx);
    view.update(cx, |view, cx| view.go(Go::Back, cx));
    assert!(host.asked(cx).contains(&AppRequest::Go { pane, to: Go::Back }));
    view.read_with(cx, |view, _| {
        assert!(view.notice.is_none(), "going back loads a page too");
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
    view.read_with(cx, |view, _| assert!(view.lease.control.person_holds()));
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    still(cx, 3);
    view.read_with(cx, |view, _| {
        assert!(view.lease.control.person_holds(), "still at it");
    });
    still(cx, 2);
    view.read_with(cx, |view, _| {
        assert!(view.lease.control.holder.is_none(), "gone back");
    });
    view.update(cx, |view, cx| view.person_acts(Mode::Explicit, cx));
    still(cx, 60);
    view.read_with(cx, |view, _| assert!(view.lease.control.person_holds()));
    view.update(cx, BrowserView::give_back);
    view.read_with(cx, |view, _| assert!(view.lease.control.holder.is_none()));
}

#[gpui_kit::test]
fn the_agents_pointer_travels_only_where_the_person_can_see_it(cx: &mut TestAppContext) {
    let (window, drawn) = start_page(cx);
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

    for (on, then) in [(true, true), (true, true), (false, false), (false, false)] {
        view.update(cx, |view, cx| view.devtools_asked(device(on), cx));
        assert_eq!(responsive(cx), then);
    }

    view.update(cx, |view, cx| view.devtools_asked(DevToolsAsk::Close, cx));
    view.read_with(cx, |view, _| assert!(view.devtools().is_none(), "closed"));

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

#[gpui_kit::test]
fn the_persons_right_click_in_a_page_an_agent_holds_takes_it_back_and_shows_its_menu(
    cx: &mut TestAppContext,
) {
    use crate::browser::control::Actor;
    use crate::native::menus::Refusal;
    use gpui_kit::{Modifiers, MouseButton, MouseDownEvent, point};

    let (window, view) = start_page(cx);
    let agent = Actor {
        id: "a".into(),
        label: "claude-code:7f3a".into(),
    };
    view.update(cx, |view, cx| view.agent_claims(agent, false, None, cx))
        .unwrap();
    let gate = |cx: &mut TestAppContext| {
        window
            .update(cx, |_, window, cx| view.read(cx).menu_gate(window))
            .unwrap()
    };
    assert_eq!(gate(cx), Err(Refusal::Agent));
    let press = MouseDownEvent {
        position: point(px(200.), px(150.)),
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    };
    window
        .update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.pressed_at(&press, window, cx));
        })
        .unwrap();
    view.read_with(cx, |view, _| assert!(view.lease.control.person_holds()));
    assert_eq!(gate(cx), Ok(()), "the person's menu shows");
}

/// A page made in a window, its host the fake one, `serve`d as `name`.
fn page_with_fake_host(
    name: &str,
    url: &str,
    before_shown: impl FnOnce(&mut BrowserView),
    cx: &mut TestAppContext,
) -> (crate::browser::host::fake::FakeHost, Entity<BrowserView>) {
    cx.update(crate::test_support::init);
    let host = crate::browser::host::fake::FakeHost::serve(name, cx);
    let url = url.to_owned();
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        cx.new(|cx| {
            let mut view = BrowserView::new(url, Path::new("/work/app"), window, cx);
            before_shown(&mut view);
            view
        })
    });
    draw(window, cx);
    (host, view)
}

/// A page brought back opens with the history and scroll it had, which the host hears just
/// before the page opens; and what the host says of them since is what the window keeps.
#[gpui_kit::test]
fn a_page_brought_back_opens_with_its_history_and_keeps_what_the_host_says(
    cx: &mut TestAppContext,
) {
    use koyo_browser_ipc::{AppRequest, HostEvent, Scroll, SessionEntry};
    let entry = |url: &str| SessionEntry {
        url: url.into(),
        title: String::new(),
    };
    let kept = Session {
        back: vec![entry("https://a.example/")],
        forward: Vec::new(),
        scroll: Scroll { x: 0, y: 900 },
    };
    let brought = kept.clone();
    let (host, view) = page_with_fake_host(
        "page-session",
        "https://b.example/",
        move |view| view.bring_back(brought),
        cx,
    );
    let pane = view.read_with(cx, |view, _| view.pane);
    let asked = host.asked(cx);
    let opened = (asked.iter()).position(|a| matches!(a, AppRequest::Open { pane: p, .. } if *p == pane));
    let brought = (asked.iter()).position(|a| *a == AppRequest::BringBack { pane, session: kept.clone() });
    assert!(
        brought.zip(opened).is_some_and(|(brought, opened)| brought < opened),
        "its history goes to the host before it opens: {asked:?}"
    );

    let told = std::rc::Rc::new(std::cell::Cell::new(0));
    let heard = std::rc::Rc::clone(&told);
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &BrowserEvent, _| {
            if matches!(event, BrowserEvent::SessionChanged) {
                heard.set(heard.get() + 1);
            }
        })
        .detach();
    });
    let later = Session {
        back: vec![entry("https://a.example/"), entry("https://b.example/")],
        ..Session::default()
    };
    host.says(HostEvent::Session { pane, session: later.clone() }, cx);
    host.says(HostEvent::Session { pane, session: later.clone() }, cx);
    view.read_with(cx, |view, _| assert_eq!(view.session(), &later));
    assert_eq!(told.get(), 1, "the window hears of a change once");
}

/// A page's zoom is the engine's for its site, as Chrome's is: zoomed, the host is asked to
/// zoom the site; and a page of another site loading isn't zoomed as the last was.
#[gpui_kit::test]
fn a_pages_zoom_is_its_sites_and_another_site_loading_isnt_zoomed_as_the_last(
    cx: &mut TestAppContext,
) {
    use koyo_browser_ipc::{AppRequest, HostEvent, ViewAction, Zoom};
    let (host, view) = page_with_fake_host("page-zoom", "https://a.example/", |_| {}, cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    let state = |url: &str, loading: bool, zoom: u32| PageState {
        url: url.into(),
        loading,
        zoom: Zoom(zoom),
        ..PageState::default()
    };
    host.says(HostEvent::Page { pane, state: state("https://a.example/", false, 100) }, cx);
    let _ = host.asked(cx);
    view.update(cx, |view, cx| view.zoom_by(1, cx));
    let zooms = |asked: Vec<AppRequest>| -> Vec<u32> {
        (asked.into_iter())
            .filter_map(|asked| match asked {
                AppRequest::View { action: ViewAction::Zoom { level }, .. } => {
                    Some(Zoom::of_level(level).0)
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(zooms(host.asked(cx)), [110], "the site is zoomed a step");
    host.says(HostEvent::Page { pane, state: state("https://a.example/", false, 110) }, cx);
    host.says(HostEvent::Page { pane, state: state("https://b.example/", true, 110) }, cx);
    host.says(HostEvent::Page { pane, state: state("https://b.example/", false, 100) }, cx);
    assert_eq!(zooms(host.asked(cx)), Vec::<u32>::new(), "nothing zooms the other site");
    view.read_with(cx, |view, _| assert_eq!(view.zoom, 100, "it shows the other site's zoom"));
}

/// A page brought back and sent elsewhere before it opened, as an agent may one whose tab
/// wasn't shown, opens there with the page it would have opened at behind it.
#[gpui_kit::test]
fn a_page_brought_back_sent_elsewhere_before_it_opens_keeps_its_place_behind(
    cx: &mut TestAppContext,
) {
    use koyo_browser_ipc::{AppRequest, Scroll, SessionEntry};
    cx.update(crate::test_support::init);
    let host = crate::browser::host::fake::FakeHost::serve("page-sent-on", cx);
    let entry = |url: &str| SessionEntry {
        url: url.into(),
        title: String::new(),
    };
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        cx.new(|cx| {
            let url = "https://b.example/".to_owned();
            let mut view = BrowserView::new(url, Path::new("/work/app"), window, cx);
            view.bring_back(Session {
                back: vec![entry("https://a.example/")],
                forward: vec![entry("https://z.example/")],
                scroll: Scroll { x: 0, y: 500 },
            });
            view.navigate("https://c.example/".into(), cx);
            view
        })
    });
    draw(window, cx);
    let pane = view.read_with(cx, |view, _| view.pane);
    let behind = Session {
        back: vec![entry("https://a.example/"), entry("https://b.example/")],
        ..Session::default()
    };
    let asked = host.asked(cx);
    assert!(
        asked.contains(&AppRequest::BringBack { pane, session: behind }),
        "{asked:?}"
    );
    let opened = asked.iter().find_map(|asked| match asked {
        AppRequest::Open { pane: p, url, .. } if *p == pane => Some(url.as_str()),
        _ => None,
    });
    assert_eq!(opened, Some("https://c.example/"));
}
