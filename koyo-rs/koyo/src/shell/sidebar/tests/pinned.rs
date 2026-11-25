//! The pinned pages at the sidebar's head (`docs/sidebar.md` 11): a page's tab dropped on
//! their region is pinned, and nothing else is; a tile dragged among them moves, and onto a
//! worktree's tabs is unpinned there; a pinned page shown stays shown, the keyboard where it was,
//! as the space changes; closed, it stays pinned; and the empty zone, put away, still shows for
//! a page's tab in hand.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, EntityId, Focusable as _, TestAppContext, point, px,
};

use super::{act, chat, drag, drag_onto, drawn, let_go_on, listed, sidebar_shell, take_up};
use crate::shell::spaces::{NewSpace, NextSpace, PreviousSpace};
use crate::model::Edge;
use crate::projects::MoveProjectToNewSpace;
use crate::settings;
use crate::shell::{
    ClosePinned, CloseTab, PinnedMenu, RemovePinned, RepinAtCurrent, Shell, Unpin,
};
use crate::test_support::{carry, click, draw, let_go, open_page, pin, pinned};

/// The address of the page the panes show, and whether it's a pinned one.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<String>, bool) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address());
        (address.map(str::to_owned), workspace.pinned_shown().is_some())
    })
}

/// The view of the pinned page known by `key`, once it's made, and whether it has the keyboard.
pub(super) fn page_of(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    key: u64,
    cx: &mut TestAppContext,
) -> Option<(EntityId, bool)> {
    for _ in 0..3 {
        draw(window, cx);
    }
    cx.update_window(window, |_, window, cx| {
        let view = shell.read(cx).workspace.pane(key)?.view()?.clone();
        let focused = view.focus_handle(cx).is_focused(window);
        Some((view.entity_id(), focused))
    })
    .unwrap()
}

#[gpui_kit::test]
fn a_pages_tab_dropped_on_the_pinned_region_is_pinned_and_no_other_kind_is(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-drop"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let docs = open_page(window, &shell, Some("https://example.com/docs"), cx);
    let app = open_page(window, &shell, Some("http://localhost:3000/"), cx);
    open_page(window, &shell, Some("https://example.com/other"), cx);
    draw(window, cx);
    assert!(
        drawn(window, "sidebar-pinned-zone", cx).is_some(),
        "with none pinned, the zone says where one goes"
    );

    drag(window, "tab-0-0-0", "sidebar-pinned", cx);
    assert!(pinned(&shell, cx).is_empty());
    assert_eq!(listed(&shell, cx).len(), 4);

    drag(window, "tab-0-0-1", "sidebar-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/docs"]);
    assert_eq!(listed(&shell, cx).len(), 3);
    assert!(drawn(window, "sidebar-pinned-0", cx).is_some());
    assert!(drawn(window, "sidebar-pinned-zone", cx).is_none());
    assert!(!shown(&shell, cx).1, "it wasn't the tab shown, and isn't");
    let home = cx.read(|cx| shell.read(cx).workspace.pinned()[0].home.clone());
    assert_eq!(home, "https://example.com/docs", "pinned where it was");
    assert_eq!(
        cx.read(|cx| shell.read(cx).workspace.pinned_at(docs)),
        Some(0)
    );

    take_up(window, "tab-0-0-2", cx);
    let tiles = drawn(window, "sidebar-pinned", cx).expect("the tiles");
    let past = point(tiles.right() - px(2.), tiles.center().y);
    carry(window, past, cx);
    let_go(window, past, cx);
    assert_eq!(
        pinned(&shell, cx),
        ["https://example.com/docs", "https://example.com/other"],
        "let go past the first, it's after it"
    );
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.com/other".into()), true)
    );

    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 1, cx));
    let beside = open_page(window, &shell, Some("https://example.com/beside"), cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        assert!(unit.drop_tab(2, app, Edge::Right));
        cx.notify();
    });
    draw(window, cx);
    drag(window, "tab-0-0-1", "sidebar-pinned", cx);
    assert_eq!(pinned(&shell, cx).len(), 2, "{beside} and {app} stay a tab");
    assert_eq!(listed(&shell, cx).len(), 2);
}

/// A page's tab let go on the tiles is pinned where the pointer has it among them, as the tiles
/// make room for it there: before the first, between two, or after the last, on any row.
#[gpui_kit::test]
fn a_pages_tab_let_go_among_the_tiles_lands_where_they_made_room_for_it(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-land"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
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
    draw(window, cx);
    let region = |cx: &mut TestAppContext| drawn(window, "sidebar-pinned", cx).expect("the tiles");

    take_up(window, "tab-0-0-1", cx);
    let tiles = region(cx);
    let first = point(tiles.left() + px(2.), tiles.top() + px(22.));
    carry(window, first, cx);
    let_go(window, first, cx);
    assert_eq!(pinned(&shell, cx), [four, one, two, three], "before the first");

    take_up(window, "tab-0-0-1", cx);
    let tiles = region(cx);
    let between = point(tiles.left() + tiles.size.width * 0.4, tiles.top() + px(22.));
    carry(window, between, cx);
    let_go(window, between, cx);
    assert_eq!(pinned(&shell, cx), [four, five, one, two, three], "between the first two");

    take_up(window, "tab-0-0-1", cx);
    let tiles = region(cx);
    let last = point(tiles.right() - px(2.), tiles.bottom() - px(2.));
    carry(window, last, cx);
    let_go(window, last, cx);
    let landed = [four, five, one, two, three, six];
    assert_eq!(pinned(&shell, cx), landed, "after the last, on the second row");
    assert_eq!(listed(&shell, cx), ["A chat"]);
}

#[gpui_kit::test]
fn the_empty_zone_put_away_still_shows_while_a_pages_tab_is_in_hand(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-zone"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);
    click(window, "sidebar-pinned-dismiss", cx);
    assert!(cx.read(|cx| settings::get(cx).pinned_zone_dismissed));
    assert!(drawn(window, "sidebar-pinned", cx).is_none(), "put away");

    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "sidebar-pinned", cx).is_none());
    let_go_on(window, "tab-0-0-0", cx);
    drag(window, "tab-0-0-1", "tab-0-0-1", cx);
    assert!(drawn(window, "sidebar-pinned", cx).is_none());
    drag_onto(window, "tab-0-0-1", "sidebar-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/"]);
    assert!(cx.read(|cx| settings::get(cx).pinned_zone_dismissed));

    let key = cx.read(|cx| shell.read(cx).workspace.pinned()[0].key());
    act(window, RemovePinned(key), cx);
    assert!(pinned(&shell, cx).is_empty());
    assert!(drawn(window, "sidebar-pinned", cx).is_none());
}

#[gpui_kit::test]
fn a_pinned_tile_moves_among_the_others_and_onto_a_worktrees_tabs_is_unpinned(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-move"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        shell.workspace.push_tab(chat("Another"));
        cx.notify();
    });
    let pages = [
        "https://a.example/",
        "https://b.example/",
        "https://c.example/",
    ];
    let keys = pin(window, &shell, &pages, cx);
    assert_eq!(pinned(&shell, cx), pages);
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), true));

    drag(window, "sidebar-pinned-0", "sidebar-pinned-2", cx);
    assert_eq!(pinned(&shell, cx), [pages[1], pages[2], pages[0]]);
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), true));

    click(window, "sidebar-pinned-0", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));

    drag_onto(window, "sidebar-pinned-2", "tab-0-0-1", cx);
    assert_eq!(pinned(&shell, cx), [pages[1], pages[2]]);
    assert_eq!(listed(&shell, cx), ["A chat", "a.example", "Another"]);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), false));

    act(window, Unpin(keys[2]), cx);
    assert_eq!(pinned(&shell, cx), [pages[1]]);
    assert_eq!(
        listed(&shell, cx).last().map(String::as_str),
        Some("c.example")
    );
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), false));
}

#[gpui_kit::test]
fn a_pinned_page_shown_stays_shown_with_the_keyboard_as_the_space_changes(cx: &mut TestAppContext) {
    let (window, shell, roots) =
        sidebar_shell(&["sidebar-pin-space-app", "sidebar-pin-space-site"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let page = "https://example.com/inbox";
    let key = pin(window, &shell, &[page], cx)[0];
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    click(window, "sidebar-pinned-0", cx);
    let (view, has_keys) = page_of(window, &shell, key, cx).expect("its page, made");
    assert!(has_keys, "picked, it takes the keyboard");
    let space = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).space);
    let project = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.project()).map(|project| project.path.clone()))
    };
    let still = |cx: &mut TestAppContext| {
        assert_eq!(shown(&shell, cx), (Some(page.into()), true));
        let now = page_of(window, &shell, key, cx);
        assert_eq!(now, Some((view, true)), "the same page, the keyboard in it");
    };

    act(window, NextSpace, cx);
    assert_eq!((space(cx), project(cx)), (1, Some(roots[1].clone())));
    still(cx);
    act(window, PreviousSpace, cx);
    assert_eq!((space(cx), project(cx)), (0, Some(roots[0].clone())));
    still(cx);
    click(window, "space-mark-1", cx);
    assert_eq!(space(cx), 1);
    still(cx);
    act(window, NewSpace, cx);
    assert_eq!((space(cx), project(cx)), (2, None));
    still(cx);
    assert_eq!(super::said(&shell, cx), "example.com");
    let steered = cx.read(|cx| (shell.read(cx).shown_page()).map(|page| page.entity_id()));
    assert_eq!(steered, Some(view));

    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    assert_eq!(drawn(window, "sidebar-pinned-0", cx).map(|_| ()), Some(()));
    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, false), "the chat shows");
    click(window, "sidebar-pinned-0", cx);
    still(cx);
    shell.update(cx, |shell, cx| shell.select_project(0, cx));
    draw(window, cx);
    assert!(!shown(&shell, cx).1);
    let alive = page_of(window, &shell, key, cx).map(|(view, _)| view);
    assert_eq!(alive, Some(view), "left, its page is still open");
}

#[gpui_kit::test]
fn a_pinned_page_closed_stays_pinned_and_goes_back_to_where_its_pinned(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-close"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let pages = ["https://a.example/", "https://b.example/"];
    let keys = pin(window, &shell, &pages, cx);
    let menu = |key: u64, cx: &TestAppContext| cx.read(|cx| PinnedMenu::of(shell.read(cx), key));
    let open = |key: u64, cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.pane(key)).is_some_and(|page| page.view().is_some()))
    };
    assert!(menu(keys[1], cx).open && !menu(keys[1], cx).strayed);

    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));
    act(window, CloseTab, cx);
    assert_eq!(pinned(&shell, cx), pages, "still pinned");
    assert_eq!(shown(&shell, cx), (None, false), "the chat shows");
    assert!(!open(keys[1], cx) && !menu(keys[1], cx).open);
    assert_eq!(listed(&shell, cx), ["A chat"], "and no tab closed with it");
    click(window, "sidebar-pinned-1", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));
    assert!(page_of(window, &shell, keys[1], cx).is_some());

    let view = cx.read(|cx| shell.read(cx).workspace.pane(keys[0])?.view().cloned());
    let view = view.expect("the first page, open since it was pinned");
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.navigate("https://a.example/later".into(), cx);
        });
    })
    .unwrap();
    assert_eq!(pinned(&shell, cx)[0], "https://a.example/later");
    assert!(menu(keys[0], cx).strayed);
    act(window, ClosePinned(keys[0]), cx);
    assert_eq!(pinned(&shell, cx), pages);
    assert!(!open(keys[0], cx));
    assert!(shown(&shell, cx).1, "the other pinned page stays shown");

    click(window, "sidebar-pinned-0", cx);
    let view = cx.read(|cx| shell.read(cx).workspace.pane(keys[0])?.view().cloned());
    cx.update_window(window, |_, _, cx| {
        (view.expect("opened again")).update(cx, |view, cx| {
            view.navigate("https://a.example/new".into(), cx);
        });
    })
    .unwrap();
    act(window, RepinAtCurrent(keys[0]), cx);
    let home = cx.read(|cx| shell.read(cx).workspace.pinned()[0].home.clone());
    assert_eq!(home, "https://a.example/new");
    assert!(!menu(keys[0], cx).strayed);
    act(window, ClosePinned(keys[0]), cx);
    assert_eq!(pinned(&shell, cx)[0], "https://a.example/new");
    shell.update(cx, |shell, cx| {
        shell.repin_at(keys[0], "https://a.example/edited".into(), cx);
    });
    assert_eq!(pinned(&shell, cx)[0], "https://a.example/edited");

    act(window, RemovePinned(keys[0]), cx);
    assert_eq!(pinned(&shell, cx), [pages[1]]);
    assert!(cx.read(|cx| shell.read(cx).workspace.pane(keys[0]).is_none()));
}

#[gpui_kit::test]
fn the_pointer_resting_on_a_pinned_tile_brings_its_pages_title_and_host(cx: &mut TestAppContext) {
    use crate::shell::sidebar::card::{self, Card};

    let (window, shell, _) = sidebar_shell(&["sidebar-pin-card"], cx);
    pin(window, &shell, &["https://mail.example.com/inbox"], cx);
    assert!(cx.read(card::shown).is_none());
    let mut vcx = gpui_kit::VisualTestContext::from_window(window, cx);
    let tile = vcx.debug_bounds("sidebar-pinned-0").expect("its tile");
    vcx.simulate_mouse_move(tile.center(), None, gpui_kit::Modifiers::default());
    draw(window, cx);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    let Some(Card::Page(page)) = cx.read(card::shown) else {
        panic!("no card for the pinned page");
    };
    assert_eq!(page.title.as_ref(), "mail.example.com/inbox");
    assert_eq!(page.host.as_ref(), "mail.example.com");
}

/// The keyboard goes by `shell/keys.rs` with these pages as with any tab: a pinned or a loose
/// page closed with the keyboard in it hands it to what shows in its place, one moved leaves
/// it where it was, and one closed with the keyboard at the window leaves it there.
#[gpui_kit::test]
fn a_page_closed_with_the_keyboard_in_it_hands_it_to_what_shows_in_its_place(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = sidebar_shell(&["sidebar-pin-close-keys"], cx);
    open_page(window, &shell, Some("https://example.com/first"), cx);
    open_page(window, &shell, Some("https://example.com/under"), cx);
    let key = pin(window, &shell, &["https://example.com/inbox"], cx)[0];
    let has_keys = |pane: u64, cx: &mut TestAppContext| {
        page_of(window, &shell, pane, cx).is_some_and(|(_, has_keys)| has_keys)
    };
    let tab_shown = |cx: &TestAppContext| {
        shell
            .read_with(cx, |shell, _| shell.shown_tab_key())
            .unwrap()
    };

    click(window, "sidebar-pinned-0", cx);
    assert!(has_keys(key, cx), "picked, it takes the keyboard");
    act(window, CloseTab, cx);
    let under = tab_shown(cx);
    assert_ne!(under, key, "closed, the worktree's tab shows in its place");
    assert!(has_keys(under, cx), "which has the keyboard");

    shell.update(cx, |shell, cx| shell.loosen_tab_of(under, usize::MAX, cx));
    assert_eq!(tab_shown(cx), under, "loose, it shows");
    assert!(has_keys(under, cx), "moved, the keyboard is where it was");
    click(window, "sidebar-unpinned-close-0", cx);
    let first = tab_shown(cx);
    assert_ne!(first, under, "closed, the worktree's other tab shows");
    assert!(has_keys(first, cx), "which has the keyboard");

    click(window, "sidebar-pinned-0", cx);
    assert!(
        has_keys(key, cx),
        "picked again, it opens and takes the keyboard"
    );
    (cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    }))
    .unwrap();
    act(window, ClosePinned(key), cx);
    assert_eq!(tab_shown(cx), first);
    assert!(!has_keys(first, cx), "the keyboard stays at the window");
}
