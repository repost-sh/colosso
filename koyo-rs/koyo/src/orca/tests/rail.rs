//! The Orca layout's top rail before the selected checkout's tabs (`docs/arc-sidebar.md` 1, 11):
//! the pinned pages as buttons in a pill, picked to show and dragged among themselves; the zone
//! that says where one goes while none is, put away and back for a page's tab in hand; a page's
//! tab let go on the pill, pinned where the buttons made room for it; the space's loose pages as
//! tabs, their + and a hairline between them and the checkout's tabs; and a page's tab let go
//! among them, or on the spot that shows where they'd be while there's none (`docs/layout.md`).

use gpui_kit::{AnyWindowHandle, Bounds, Entity, Pixels, Point, TestAppContext, point, px};

use crate::settings;
use crate::shell::Shell;
use crate::test_support::{
    carry, click, drawn, hold, let_go, let_go_on, loose, open_page, orca_on_chat, pin, pinned,
    take_up,
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

/// Drags what's at `from` to `to` and lets it go there, drawing at each step.
fn drag(
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
    let (window, shell) = orca_on_chat("orca-rail-pins", cx);
    let pages = ["https://a.example/", "https://b.example/", "https://c.example/"];
    pin(window, &shell, &pages, cx);
    let pill = drawn(window, "orca-pinned", cx).expect("the pinned pages' pill");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!(in_rail(pill) && pill.right() < chat.left(), "{pill:?} before {chat:?}");
    for at in 0..pages.len() {
        let button = drawn(window, format!("orca-pinned-{at}"), cx).expect("a pinned button");
        assert!(pill.contains(&button.center()), "{button:?} in {pill:?}");
    }

    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, None, None), "the chat shows");
    click(window, "orca-pinned-0", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), Some(0), None));
    let checkout = cx.read(|cx| shell.read(cx).workspace.shown_checkout_tab());
    assert_eq!(checkout, None, "no checkout's tab is the one shown");

    let from = drawn(window, "orca-pinned-0", cx).expect("the first").center();
    let to = drawn(window, "orca-pinned-2", cx).expect("the last").center();
    drag(window, from, to, cx);
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
    let (window, shell) = orca_on_chat("orca-rail-zone", cx);
    open_page(window, &shell, Some("https://example.com/"), cx);
    let zone = drawn(window, "orca-pinned-zone", cx).expect("the zone");
    assert!(in_rail(zone), "{zone:?}");
    let dismissed = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).pinned_zone_dismissed);

    click(window, "orca-pinned-dismiss", cx);
    assert!(dismissed(cx));
    assert!(drawn(window, "orca-pinned", cx).is_none(), "put away");

    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "orca-pinned", cx).is_none(), "a chat's tab isn't pinned");
    let_go_on(window, "tab-0-0-0", cx);

    take_up(window, "tab-0-0-1", cx);
    assert!(drawn(window, "orca-pinned-zone", cx).is_some(), "back for the page's tab");
    let_go_on(window, "orca-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/"]);
    assert!(dismissed(cx), "still put away for when none is pinned");
}

/// A page's tab let go on the pill is pinned where the pointer has it among the buttons, which
/// make room for it there: before the first, between two, after the last.
#[gpui_kit::test]
fn a_pages_tab_let_go_on_the_pill_lands_where_the_buttons_made_room_for_it(
    cx: &mut TestAppContext,
) {
    let (window, shell) = orca_on_chat("orca-rail-land", cx);
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
    let first = drawn(window, "orca-pinned-0", cx).expect("the first button");
    let before = point(first.left() + px(2.), first.center().y);
    carry(window, before, cx);
    let_go(window, before, cx);
    assert_eq!(pinned(&shell, cx), [four, one, two, three], "before the first");

    take_up(window, "tab-0-0-1", cx);
    let second = drawn(window, "orca-pinned-1", cx).expect("the second button");
    carry(window, second.center(), cx);
    let stepped = drawn(window, "orca-pinned-1", cx).expect("the second button");
    let room = stepped.left() - second.left();
    assert!(room > px(20.), "it made room: {second:?} to {stepped:?}");
    let_go(window, second.center(), cx);
    assert_eq!(pinned(&shell, cx), [four, five, one, two, three], "between the first two");

    take_up(window, "tab-0-0-1", cx);
    let pill = drawn(window, "orca-pinned", cx).expect("the pill");
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
    let (window, shell) = orca_on_chat("orca-rail-loose", cx);
    cx.update(|cx| settings::update(cx, |settings| settings.pinned_zone_dismissed = true));
    let news = open_page(window, &shell, Some("https://news.example/"), cx);
    let maps = open_page(window, &shell, Some("https://maps.example/"), cx);
    shell.update(cx, |shell, cx| {
        shell.loosen_tab_of(news, usize::MAX, cx);
        shell.loosen_tab_of(maps, usize::MAX, cx);
    });
    let first = drawn(window, "orca-loose-0", cx).expect("the first loose page's tab");
    let second = drawn(window, "orca-loose-1", cx).expect("the second's");
    let plus = drawn(window, "orca-new-page", cx).expect("their +");
    let hairline = drawn(window, "orca-rail-separator", cx).expect("the hairline");
    let chat = drawn(window, "tab-0-0-0", cx).expect("the chat's tab");
    assert!([first, second, plus, hairline, chat].into_iter().all(in_rail));
    assert!(first.right() <= second.left(), "{first:?} {second:?}");
    assert!(second.right() <= plus.left(), "{second:?} {plus:?}");
    assert!(plus.right() <= hairline.left(), "{plus:?} {hairline:?}");
    assert!(hairline.right() <= chat.left(), "{hairline:?} {chat:?}");

    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, None, None));
    click(window, "orca-loose-0", cx);
    let news_shown = (Some("https://news.example/".to_owned()), None, Some(0));
    assert_eq!(shown(&shell, cx), news_shown);

    let past = point(second.center().x + px(10.), second.center().y);
    drag(window, first.center(), past, cx);
    assert_eq!(loose(&shell, cx), ["https://maps.example/", "https://news.example/"]);

    click(window, "orca-loose-close-0", cx);
    assert!(drawn(window, "orca-rail-separator", cx).is_some(), "one left");
    click(window, "orca-loose-close-0", cx);
    assert!(cx.read(|cx| shell.read(cx).workspace.loose().is_empty()));
    assert!(drawn(window, "orca-new-page", cx).is_some(), "none left, the + stays");
    assert!(drawn(window, "orca-rail-separator", cx).is_some(), "and the hairline before the chat");
}

/// With no loose pages, a dashed spot shows where they'd be, before their +, only while a page's
/// tab is in hand off the checkout's list: let go there, it's the last loose page, and shows.
/// With some, a page's tab let go on one takes its place among them, and shows.
#[gpui_kit::test]
fn a_pages_tab_let_go_on_the_spot_is_the_last_loose_page_and_shows(cx: &mut TestAppContext) {
    let (window, shell) = orca_on_chat("orca-rail-spot", cx);
    let (news, maps) = ("https://news.example/", "https://maps.example/");
    open_page(window, &shell, Some(news), cx);
    open_page(window, &shell, Some(maps), cx);
    click(window, "tab-0-0-0", cx);
    assert!(drawn(window, "orca-loose-spot", cx).is_none(), "nothing in hand");

    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "orca-loose-spot", cx).is_none(), "a chat's tab isn't kept there");
    let_go_on(window, "tab-0-0-0", cx);

    take_up(window, "tab-0-0-1", cx);
    let spot = drawn(window, "orca-loose-spot", cx).expect("the spot, for the page in hand");
    let plus = drawn(window, "orca-new-page", cx).expect("the loose pages' +");
    assert!(in_rail(spot) && spot.right() <= plus.left(), "{spot:?} before {plus:?}");
    let_go_on(window, "orca-loose-spot", cx);
    assert_eq!(loose(&shell, cx), [news]);
    assert_eq!(shown(&shell, cx), (Some(news.into()), None, Some(0)), "shown");
    assert!(drawn(window, "orca-loose-spot", cx).is_none(), "gone with a loose page");
    let tabs = cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    assert_eq!(tabs, Some(2), "off the checkout's tabs");

    take_up(window, "tab-0-0-1", cx);
    let_go_on(window, "orca-loose-0", cx);
    assert_eq!(loose(&shell, cx), [maps, news], "in the place of the one it's let go on");
    assert_eq!(shown(&shell, cx), (Some(maps.into()), None, Some(0)));
}
