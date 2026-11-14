//! The pinned pages at the sidebar's head (`docs/arc-sidebar.md` 11): a page's tab dropped on
//! their region is pinned, and nothing else is; a tile dragged among them moves, and onto a
//! worktree's tabs is unpinned there; a pinned page shown stays shown, the keyboard where it was,
//! as the space changes; closed, it stays pinned; and the empty zone, put away, still shows for
//! a page's tab in hand.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, EntityId, Focusable as _, TestAppContext,
};

use super::{act, arc_shell, chat, drag, drag_onto, drawn, let_go_on, listed, take_up};
use crate::arc::actions::{
    ClosePinned, MoveProjectToNewSpace, NewSpace, NextSpace, PreviousSpace, RemovePinned,
    RepinAtCurrent, Unpin,
};
use crate::arc::pinned::Menu;
use crate::model::Edge;
use crate::settings;
use crate::shell::{CloseTab, Shell};
use crate::test_support::{click, draw, open_page};

/// The pinned pages, by the addresses they show, in the order of their tiles.
fn pinned(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (shell.read(cx).workspace.pinned().iter())
            .map(|pinned| pinned.tab.first().address(cx).unwrap_or_default())
            .collect()
    })
}

/// The address of the page the panes show, and whether it's a pinned one.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<String>, bool) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address(cx));
        (address, workspace.pinned_shown().is_some())
    })
}

/// Opens a page at each of `addresses` on the selected checkout and pins them, in that order:
/// what their tiles are known by.
fn pin(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    addresses: &[&str],
    cx: &mut TestAppContext,
) -> Vec<u64> {
    let keys: Vec<u64> = (addresses.iter())
        .map(|address| open_page(window, shell, Some(address), cx))
        .collect();
    shell.update(cx, |shell, cx| {
        for key in &keys {
            assert!(shell.pin_tab_of(*key, cx), "a page's tab is pinned");
        }
    });
    draw(window, cx);
    keys
}

/// The view of the pinned page known by `key`, once it's made, and whether it has the keyboard.
fn page_of(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    key: u64,
    cx: &mut TestAppContext,
) -> Option<(EntityId, bool)> {
    // Drawn, the keyboard's loss is heard; drawn again, the tab shown takes it.
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
    let (window, shell, _) = arc_shell(&["arc-pin-drop"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let docs = open_page(window, &shell, Some("https://example.com/docs"), cx);
    let app = open_page(window, &shell, Some("http://localhost:3000/"), cx);
    open_page(window, &shell, Some("https://example.com/other"), cx);
    draw(window, cx);
    assert!(
        drawn(window, "arc-pinned-zone", cx).is_some(),
        "with none pinned, the zone says where one goes"
    );

    // A chat's tab isn't taken.
    drag(window, "tab-0-0-0", "arc-pinned", cx);
    assert!(pinned(&shell, cx).is_empty());
    assert_eq!(listed(&shell, cx).len(), 4);

    // A page's is: it leaves its worktree's list, and the zone gives way to its tile.
    drag(window, "tab-0-0-1", "arc-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/docs"]);
    assert_eq!(listed(&shell, cx).len(), 3);
    assert!(drawn(window, "arc-pinned-0", cx).is_some());
    assert!(drawn(window, "arc-pinned-zone", cx).is_none());
    assert!(!shown(&shell, cx).1, "it wasn't the tab shown, and isn't");
    let home = cx.read(|cx| shell.read(cx).workspace.pinned()[0].home.clone());
    assert_eq!(home, "https://example.com/docs", "pinned where it was");
    assert_eq!(
        cx.read(|cx| shell.read(cx).workspace.pinned_at(docs)),
        Some(0)
    );

    // The tab shown, pinned, goes on showing: dropped on the tiles, it's the last of them.
    drag(window, "tab-0-0-2", "arc-pinned", cx);
    assert_eq!(
        pinned(&shell, cx),
        ["https://example.com/docs", "https://example.com/other"]
    );
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.com/other".into()), true)
    );

    // Two pages side by side are a split, which isn't taken. Back on a tab of the worktree's,
    // where a page opened is the worktree's.
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 1, cx));
    let beside = open_page(window, &shell, Some("https://example.com/beside"), cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout");
        assert!(unit.drop_tab(2, app, Edge::Right));
        cx.notify();
    });
    draw(window, cx);
    drag(window, "tab-0-0-1", "arc-pinned", cx);
    assert_eq!(pinned(&shell, cx).len(), 2, "{beside} and {app} stay a tab");
    assert_eq!(listed(&shell, cx).len(), 2);
}

#[gpui_kit::test]
fn the_empty_zone_put_away_still_shows_while_a_pages_tab_is_in_hand(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-pin-zone"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);
    click(window, "arc-pinned-dismiss", cx);
    assert!(cx.read(|cx| settings::get(cx).pinned_zone_dismissed));
    assert!(drawn(window, "arc-pinned", cx).is_none(), "put away");

    // A chat's tab taken off its list brings no zone.
    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "arc-pinned", cx).is_none());
    let_go_on(window, "tab-0-0-0", cx);
    // Nor does a page's tab moved along its own list; taken off it, it does, and the zone
    // takes it.
    drag(window, "tab-0-0-1", "tab-0-0-1", cx);
    assert!(drawn(window, "arc-pinned", cx).is_none());
    drag_onto(window, "tab-0-0-1", "arc-pinned", cx);
    assert_eq!(pinned(&shell, cx), ["https://example.com/"]);
    assert!(cx.read(|cx| settings::get(cx).pinned_zone_dismissed));

    // Removed again, there's none pinned and the zone stays away.
    let key = cx.read(|cx| shell.read(cx).workspace.pinned()[0].key());
    act(window, RemovePinned(key), cx);
    assert!(pinned(&shell, cx).is_empty());
    assert!(drawn(window, "arc-pinned", cx).is_none());
}

#[gpui_kit::test]
fn a_pinned_tile_moves_among_the_others_and_onto_a_worktrees_tabs_is_unpinned(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = arc_shell(&["arc-pin-move"], cx);
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
    // The last pinned was the tab shown, and is still.
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), true));

    // The first tile onto the last: the others step back a slot, and the one shown stays shown.
    drag(window, "arc-pinned-0", "arc-pinned-2", cx);
    assert_eq!(pinned(&shell, cx), [pages[1], pages[2], pages[0]]);
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), true));

    // A press and release without a move picks it.
    click(window, "arc-pinned-0", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));

    // Taken well away, onto the second tab's row: unpinned where that tab is, and shown there.
    drag_onto(window, "arc-pinned-2", "tab-0-0-1", cx);
    assert_eq!(pinned(&shell, cx), [pages[1], pages[2]]);
    assert_eq!(listed(&shell, cx), ["A chat", "a.example", "Another"]);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), false));

    // Onto the worktree's header, it's the first of its tabs.
    drag_onto(window, "arc-pinned-0", "arc-header-0", cx);
    assert_eq!(pinned(&shell, cx), [pages[2]]);
    assert_eq!(
        listed(&shell, cx),
        ["b.example", "A chat", "a.example", "Another"]
    );
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), false));

    // From its menu, after the worktree's tabs.
    act(window, Unpin(keys[2]), cx);
    assert!(pinned(&shell, cx).is_empty());
    assert_eq!(
        listed(&shell, cx).last().map(String::as_str),
        Some("c.example")
    );
    assert_eq!(shown(&shell, cx), (Some(pages[2].into()), false));
}

#[gpui_kit::test]
fn a_pinned_page_shown_stays_shown_with_the_keyboard_as_the_space_changes(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-pin-space-app", "arc-pin-space-site"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let page = "https://example.com/inbox";
    let key = pin(window, &shell, &[page], cx)[0];
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    // Its tile picked, its page has the keyboard.
    click(window, "arc-pinned-0", cx);
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

    // ⌥⇧→, as a swipe goes: the sidebar lists the other space's project, the panes don't
    // change.
    act(window, NextSpace, cx);
    assert_eq!((space(cx), project(cx)), (1, Some(roots[1].clone())));
    still(cx);
    act(window, PreviousSpace, cx);
    assert_eq!((space(cx), project(cx)), (0, Some(roots[0].clone())));
    still(cx);
    // A click on a space's mark, which doesn't take the keyboard from it.
    click(window, "arc-space-1", cx);
    assert_eq!(space(cx), 1);
    still(cx);
    // An empty space, which shows no project: the pinned page still shows.
    act(window, NewSpace, cx);
    assert_eq!((space(cx), project(cx)), (2, None));
    still(cx);
    // The address bar says it, and its controls and ⌘L act on it.
    let said = cx.read(|cx| crate::arc::top::said(shell.read(cx), cx).0.to_string());
    assert_eq!(said, "example.com");
    let steered = cx.read(|cx| (shell.read(cx).shown_page(cx)).map(|page| page.entity_id()));
    assert_eq!(steered, Some(view));

    // A worktree's tab picked leaves it; its tile picked again shows it as it was.
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);
    assert_eq!(drawn(window, "arc-pinned-0", cx).map(|_| ()), Some(()));
    click(window, "tab-0-0-0", cx);
    assert_eq!(shown(&shell, cx), (None, false), "the chat shows");
    click(window, "arc-pinned-0", cx);
    still(cx);
    // So does its project's tile.
    click(window, "arc-tile-0", cx);
    assert!(!shown(&shell, cx).1);
    let alive = page_of(window, &shell, key, cx).map(|(view, _)| view);
    assert_eq!(alive, Some(view), "left, its page is still open");
}

#[gpui_kit::test]
fn a_pinned_page_closed_stays_pinned_and_goes_back_to_where_its_pinned(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-pin-close"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let pages = ["https://a.example/", "https://b.example/"];
    let keys = pin(window, &shell, &pages, cx);
    let menu = |key: u64, cx: &TestAppContext| cx.read(|cx| Menu::of(shell.read(cx), key, cx));
    let open = |key: u64, cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.pane(key)).is_some_and(|page| page.view().is_some()))
    };
    assert!(menu(keys[1], cx).open && !menu(keys[1], cx).strayed);

    // ⌘W on the pinned page shown: its page goes, its tile stays, and the worktree's tab shows.
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));
    act(window, CloseTab, cx);
    assert_eq!(pinned(&shell, cx), pages, "still pinned");
    assert_eq!(shown(&shell, cx), (None, false), "the chat shows");
    assert!(!open(keys[1], cx) && !menu(keys[1], cx).open);
    assert_eq!(listed(&shell, cx), ["A chat"], "and no tab closed with it");
    // Its tile picked, it opens again where it's pinned.
    click(window, "arc-pinned-1", cx);
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));
    assert!(page_of(window, &shell, keys[1], cx).is_some());

    // Gone on to another address, Close from its menu takes it back to the one it's pinned at.
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

    // Replace Pinned URL with Current pins it where it is.
    click(window, "arc-pinned-0", cx);
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
    // Edit…: an address typed is where it's pinned, and where it opens, closed as it is.
    shell.update(cx, |shell, cx| {
        shell.repin_at(keys[0], "https://a.example/edited".into(), cx);
    });
    assert_eq!(pinned(&shell, cx)[0], "https://a.example/edited");

    // Removed, it's pinned no longer.
    act(window, RemovePinned(keys[0]), cx);
    assert_eq!(pinned(&shell, cx), [pages[1]]);
    assert!(cx.read(|cx| shell.read(cx).workspace.pane(keys[0]).is_none()));
}

#[gpui_kit::test]
fn the_pointer_resting_on_a_pinned_tile_brings_its_pages_title_and_host(cx: &mut TestAppContext) {
    use crate::arc::card::{self, Card};

    let (window, shell, _) = arc_shell(&["arc-pin-card"], cx);
    pin(window, &shell, &["https://mail.example.com/inbox"], cx);
    assert!(cx.read(card::shown).is_none());
    let mut vcx = gpui_kit::VisualTestContext::from_window(window, cx);
    let tile = vcx.debug_bounds("arc-pinned-0").expect("its tile");
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
    let (window, shell, _) = arc_shell(&["arc-pin-close-keys"], cx);
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

    // ⌘W with the keyboard in the pinned page: the worktree's tab under it has the keyboard.
    click(window, "arc-pinned-0", cx);
    assert!(has_keys(key, cx), "picked, it takes the keyboard");
    act(window, CloseTab, cx);
    let under = tab_shown(cx);
    assert_ne!(under, key, "closed, the worktree's tab shows in its place");
    assert!(has_keys(under, cx), "which has the keyboard");

    // Moved among the loose pages, that page keeps the keyboard; its × hands it on.
    shell.update(cx, |shell, cx| shell.loosen_tab_of(under, usize::MAX, cx));
    assert_eq!(tab_shown(cx), under, "loose, it shows");
    assert!(has_keys(under, cx), "moved, the keyboard is where it was");
    click(window, "arc-loose-close-0", cx);
    let first = tab_shown(cx);
    assert_ne!(first, under, "closed, the worktree's other tab shows");
    assert!(has_keys(first, cx), "which has the keyboard");

    // Closed from its menu with the keyboard at the window, the pinned page leaves it there.
    click(window, "arc-pinned-0", cx);
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
