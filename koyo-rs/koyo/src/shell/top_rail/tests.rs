//! The top rail before the selected checkout's tabs (`docs/sidebar.md` 1, 11): the pinned pages as
//! buttons in a pill, picked to show and dragged among themselves; the zone that says where one
//! goes while none is, put away and back for a page's tab in hand; a page's tab let go on the pill,
//! pinned where the buttons made room for it; the space's loose pages as tabs, their + and a
//! hairline between them and the checkout's tabs; and a page's tab let go among them, or on the
//! spot that shows where they'd be while there's none (`docs/layout.md`).

use gpui_kit::{
    AnyWindowHandle, Bounds, CursorStyle, Entity, Pixels, Point, TestAppContext, TouchPhase, point,
    px,
};

use koyo_agent::protocol::model::{Beside, Status, ThreadSummary};

use crate::model::tests::chat;
use crate::projects::MoveProjectToNewSpace;
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{
    act, carry, click, cursor_at, draw, drawn, fingers, hold, let_go, let_go_on, loose, open_page,
    pin, pinned, repo, select, shell_on_chat, shell_with, take_up,
};
use crate::theme::TITLE_BAR_HEIGHT;

/// Whether `bounds` lie in the top rail.
fn in_rail(bounds: Bounds<Pixels>) -> bool {
    bounds.top() >= px(0.) && bounds.bottom() <= px(TITLE_BAR_HEIGHT)
}

/// The address of the page the panes show, the pinned page shown and the loose page shown, by
/// their places.
fn shown(
    shell: &Entity<Shell>,
    cx: &TestAppContext,
) -> (Option<String>, Option<usize>, Option<usize>) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address());
        let (pinned, loose) = (workspace.pinned_shown(), workspace.loose_shown());
        (address.map(str::to_owned), pinned, loose)
    })
}

/// Drags what's at `from` to `to` and lets it go there, drawing at each step, so what it's over
/// has made room for it by then.
fn drag_drawn(
    window: AnyWindowHandle,
    from: Point<Pixels>,
    to: Point<Pixels>,
    cx: &mut TestAppContext,
) {
    hold(window, from, cx);
    carry(window, to, cx);
    let_go(window, to, cx);
}

/// The pinned pages are buttons in a pill in the rail, before the checkout's tabs: one picked
/// shows its page, in place of the checkout's tab, and one dragged along the pill moves there.
#[gpui_kit::test]
fn the_pinned_pages_sit_in_a_pill_in_the_rail_and_one_picked_shows(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("rail-pins", cx);
    let pages = ["https://a.example/", "https://b.example/", "https://c.example/"];
    pin(window, &shell, &pages, cx);
    let pill = drawn(window, "rail-pinned", cx).expect("the pinned pages' pill");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!(in_rail(pill) && pill.right() < chat.left(), "{pill:?} before {chat:?}");
    for at in 0..pages.len() {
        let button = drawn(window, format!("rail-pinned-{at}"), cx).expect("a pinned button");
        assert!(pill.contains(&button.center()), "{button:?} in {pill:?}");
    }

    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, None, None), "the chat shows");
    click(window, "rail-pinned-0", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), Some(0), None));
    let checkout = cx.read(|cx| shell.read(cx).workspace.shown_checkout_tab());
    assert_eq!(checkout, None, "no checkout's tab is the one shown");

    let from = drawn(window, "rail-pinned-0", cx).expect("the first").center();
    let to = drawn(window, "rail-pinned-2", cx).expect("the last").center();
    drag_drawn(window, from, to, cx);
    assert_eq!(pinned(&shell, cx), [pages[1], pages[2], pages[0]]);
    assert_eq!(shown(&shell, cx).1, Some(2), "the one shown stays shown");

    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, None, None), "a checkout's tab picked leaves it");
}

/// With none pinned, a dashed zone in the rail says where one goes; its × puts it away, and it's
/// back while a page's tab is in hand, to let go on, and stays away once there are pins again.
#[gpui_kit::test]
fn with_none_pinned_a_zone_says_where_one_goes_and_comes_back_for_a_tab_in_hand(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell_on_chat("rail-zone", cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    let zone = drawn(window, "rail-pinned-zone", cx).expect("the zone");
    assert!(in_rail(zone), "{zone:?}");
    let dismissed = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).pinned_zone_dismissed);

    click(window, "rail-pinned-dismiss", cx);
    assert!(dismissed(cx));
    assert!(drawn(window, "rail-pinned", cx).is_none(), "put away");

    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "rail-pinned", cx).is_none(), "a chat's tab isn't pinned");
    let_go_on(window, "tab-0-0-0", cx);

    take_up(window, "tab-0-0-1", cx);
    assert!(drawn(window, "rail-pinned-zone", cx).is_some(), "back for the page's tab");
    let_go_on(window, "rail-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/"]);
    assert!(dismissed(cx), "still put away for when none is pinned");
}

/// A page's tab let go on the pill is pinned where the pointer has it among the buttons, which
/// make room for it there: before the first, between two, after the last.
#[gpui_kit::test]
fn a_pages_tab_let_go_on_the_pill_lands_where_the_buttons_made_room_for_it(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell_on_chat("rail-land", cx);
    let [one, two, three, four, five, six] = [
        "https://one.example/",
        "https://two.example/",
        "https://three.example/",
        "https://four.example/",
        "https://five.example/",
        "https://six.example/",
    ];
    pin(window, &shell, &[one, two, three], cx);
    click(window, "tab-0-0-0", cx);
    for address in [four, five, six] {
        open_page(window, &shell, Some(address), cx);
    }

    take_up(window, "tab-0-0-1", cx);
    let first = drawn(window, "rail-pinned-0", cx).expect("the first button");
    let before = point(first.left() + px(2.), first.center().y);
    carry(window, before, cx);
    let_go(window, before, cx);
    assert_eq!(pinned(&shell, cx), [four, one, two, three], "before the first");

    take_up(window, "tab-0-0-1", cx);
    let second = drawn(window, "rail-pinned-1", cx).expect("the second button");
    carry(window, second.center(), cx);
    let stepped = drawn(window, "rail-pinned-1", cx).expect("the second button");
    let room = stepped.left() - second.left();
    assert!(room > px(20.), "it made room: {second:?} to {stepped:?}");
    let_go(window, second.center(), cx);
    assert_eq!(pinned(&shell, cx), [four, five, one, two, three], "between the first two");

    take_up(window, "tab-0-0-1", cx);
    let pill = drawn(window, "rail-pinned", cx).expect("the pill");
    let after = point(pill.right() - px(1.), pill.center().y);
    carry(window, after, cx);
    let_go(window, after, cx);
    assert_eq!(pinned(&shell, cx), [four, five, one, two, three, six], "after the last");
}

/// The space's loose pages are tabs after back and forward and before the checkout's tabs, their
/// + and a hairline between: picked, one shows; dragged, it moves among them; closed, it's gone,
/// and with none left the + and the hairline stay, as a checkout is selected.
#[gpui_kit::test]
fn the_spaces_loose_pages_come_before_the_checkouts_tabs_a_hairline_between(
    cx: &mut TestAppContext,
) {
    let (window, shell) = shell_on_chat("rail-loose", cx);
    cx.update(|cx| settings::update(cx, |settings| settings.pinned_zone_dismissed = true));
    let news = open_page(window, &shell, Some("https://news.example/"), cx);
    let maps = open_page(window, &shell, Some("https://maps.example/"), cx);
    shell.update(cx, |shell, cx| {
        shell.loosen_tab_of(news, usize::MAX, cx);
        shell.loosen_tab_of(maps, usize::MAX, cx);
    });
    let first = drawn(window, "rail-unpinned-0", cx).expect("the first loose page's tab");
    let second = drawn(window, "rail-unpinned-1", cx).expect("the second's");
    let plus = drawn(window, "rail-new-page", cx).expect("their +");
    let hairline = drawn(window, "rail-separator", cx).expect("the hairline");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!([first, second, plus, hairline, chat].into_iter().all(in_rail));
    assert!(first.right() <= second.left(), "{first:?} {second:?}");
    assert!(second.right() <= plus.left(), "{second:?} {plus:?}");
    assert!(plus.right() <= hairline.left(), "{plus:?} {hairline:?}");
    assert!(hairline.right() <= chat.left(), "{hairline:?} {chat:?}");

    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, None, None));
    click(window, "rail-unpinned-0", cx);
    let news_shown = (Some("https://news.example/".to_owned()), None, Some(0));
    assert_eq!(shown(&shell, cx), news_shown);

    let past = point(second.center().x + px(10.), second.center().y);
    drag_drawn(window, first.center(), past, cx);
    assert_eq!(loose(&shell, cx), ["https://maps.example/", "https://news.example/"]);

    click(window, "rail-unpinned-close-0", cx);
    assert!(drawn(window, "rail-separator", cx).is_some(), "one left");
    click(window, "rail-unpinned-close-0", cx);
    assert!(cx.read(|cx| shell.read(cx).workspace.loose().is_empty()));
    assert!(drawn(window, "rail-new-page", cx).is_some(), "none left, the + stays");
    assert!(drawn(window, "rail-separator", cx).is_some(), "and the hairline before the chat");
}

/// With no loose pages, a dashed spot shows where they'd be, before their +, only while a page's
/// tab is in hand off the checkout's list: let go there, it's the last loose page, and shows.
/// With some, a page's tab let go on one takes its place among them, and shows.
#[gpui_kit::test]
fn a_pages_tab_let_go_on_the_spot_is_the_last_loose_page_and_shows(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("rail-spot", cx);
    let (news, maps) = ("https://news.example/", "https://maps.example/");
    open_page(window, &shell, Some(news), cx);
    open_page(window, &shell, Some(maps), cx);
    click(window, "tab-0-0-0", cx);
    assert!(drawn(window, "rail-unpinned-spot", cx).is_none(), "nothing in hand");

    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "rail-unpinned-spot", cx).is_none(), "a chat's tab isn't kept there");
    let_go_on(window, "tab-0-0-0", cx);

    take_up(window, "tab-0-0-1", cx);
    let spot = drawn(window, "rail-unpinned-spot", cx).expect("the spot, for the page in hand");
    let plus = drawn(window, "rail-new-page", cx).expect("the loose pages' +");
    assert!(in_rail(spot) && spot.right() <= plus.left(), "{spot:?} before {plus:?}");
    let_go_on(window, "rail-unpinned-spot", cx);
    assert_eq!(loose(&shell, cx), [news]);
    assert_eq!(shown(&shell, cx), (Some(news.into()), None, Some(0)), "shown");
    assert!(drawn(window, "rail-unpinned-spot", cx).is_none(), "gone with a loose page");
    let tabs = cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    assert_eq!(tabs, Some(2), "off the checkout's tabs");

    take_up(window, "tab-0-0-1", cx);
    let_go_on(window, "rail-unpinned-0", cx);
    assert_eq!(loose(&shell, cx), [maps, news], "in the place of the one it's let go on");
    assert_eq!(shown(&shell, cx), (Some(maps.into()), None, Some(0)));
}

/// As two fingers pull the space away, the checkout's tabs slide with it within their own place on
/// the rail, which stays put: one pulled as far as the pinned pages' zone is no longer there, and
/// the zone, which stays, is what's under the pointer at its place (`docs/layout.md`, Motion).
#[gpui_kit::test]
fn the_checkouts_tabs_slide_within_their_own_place_never_over_the_pinned_zone(
    cx: &mut TestAppContext,
) {
    let roots = ["rail-slide-here", "rail-slide-next"].map(|name| repo(name, &[("a", "a\n")]));
    let (window, shell) = shell_with(&[&roots[0], &roots[1]], cx);
    shell.update(cx, Shell::save_projects);
    select(&shell, &roots[0], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    let zone = drawn(window, "rail-pinned-zone", cx).expect("the zone where a pinned page goes");
    let at = point(zone.left() + px(30.), zone.center().y);
    assert_eq!(cursor_at(window, at, cx), CursorStyle::Arrow, "the zone, at rest");
    let tab = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    let pull = (at.x - tab.center().x).as_f32();

    fingers(window, TouchPhase::Started, 0., cx);
    for _ in 0..4 {
        fingers(window, TouchPhase::Moved, pull / 4., cx);
    }
    let slid = drawn(window, "tab-0-0-0", cx).expect("the chat's tab, sliding");
    assert!(slid.contains(&at), "the tab slid as far as the zone: {slid:?}, {at:?}");
    assert_eq!(
        cursor_at(window, at, cx),
        CursorStyle::Arrow,
        "the zone is still what's there, the tab clipped to its own place"
    );
}

/// A chat's tab on the rail says its work and what it has for the person beside each other, how
/// many subagents work after the spinner, its title after them all.
#[gpui_kit::test]
fn a_chats_tab_says_its_work_and_what_waits_side_by_side(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("rail-sign", cx);
    let (root, pane) = cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("a checkout");
        (unit.worktree.path.clone(), unit.tabs[0].first().id)
    });
    let summary = ThreadSummary {
        status: Status::Working,
        unread: true,
        beside: Beside {
            subagents: 3,
            tasks: 0,
        },
        ..crate::chat::tests::summary("0001", &root, "A chat", true)
    };
    shell.update(cx, |shell, cx| {
        shell.workspace.set_chat(pane, summary.id.clone());
        shell.workspace.push_tab(chat("another"));
        cx.notify();
    });
    crate::chat::tests::tell(&shell, summary, cx);
    let [dot, spinner, count] = ["dot", "spinner", "subagents"].map(|part| {
        drawn(window, format!("tab-{pane}-{part}"), cx).unwrap_or_else(|| panic!("its {part}"))
    });
    assert!(in_rail(spinner));
    assert!(dot.right() < spinner.left(), "{dot:?} {spinner:?}");
    assert!(count.right() > spinner.right(), "{spinner:?} {count:?}");
    let close = drawn(window, "tab-0-0-0", cx).expect("its tab");
    assert!(count.right() < close.right(), "{count:?} in {close:?}");
}
