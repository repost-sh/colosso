//! The loose pages of a space (`docs/arc-sidebar.md` 11): a page opened where no project is
//! shown is one, and so is one opened from a pinned or a loose page; each space lists its own;
//! a page's tab let go on them is one, a loose page let go on a worktree's tabs is its tab, and
//! on the pinned pages is pinned; a space with no project shows no dot; and a loose or a pinned
//! page's menu copies it into the selected worktree.

use gpui_kit::{
    AppContext as _, Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};

use super::{act, arc_shell, chat, drag, drag_onto, drawn, let_go_on, listed, take_up};
use crate::arc::actions::{NewSpace, NextSpace, PreviousSpace};
use crate::files::tests::{dialog, press};
use crate::launcher::{Launcher, NewTab};
use crate::settings::{self, LoosePages};
use crate::shell::{CloseTab, CopyToWorktree, PinnedMenu, ReopenClosedTab, Shell};
use crate::test_support::{click, draw, open_page};

/// The loose pages of the space shown, by the addresses they show, in the order of their rows.
fn loose(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (shell.read(cx).workspace.loose().into_iter())
            .map(|page| page.tab.first().address().unwrap_or_default().to_owned())
            .collect()
    })
}

/// The address of the page the panes show, and whether it's a loose one.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<String>, bool) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address());
        (address.map(str::to_owned), workspace.loose_shown().is_some())
    })
}

#[gpui_kit::test]
fn a_page_opened_where_no_project_is_shown_is_a_loose_page_of_that_space(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-loose-open"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    draw(window, cx);
    assert!(
        drawn(window, "arc-project-name", cx).is_some(),
        "the project's name, after its space's dot"
    );
    act(window, NewSpace, cx);
    let project = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.project()).map(|project| project.path.clone()))
    };
    assert_eq!(project(cx), None);
    assert!(drawn(window, "arc-project-name", cx).is_none());

    act(window, NewTab, cx);
    let (launcher, _) = dialog::<Launcher>(cx).expect("the launcher, with no project shown");
    press(launcher, "tab", cx);
    cx.simulate_input(launcher, "example.org/inbox");
    press(launcher, "enter", cx);
    draw(window, cx);
    assert_eq!(loose(&shell, cx), ["https://example.org/inbox"]);
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.org/inbox".into()), true)
    );
    assert!(drawn(window, "arc-loose-0", cx).is_some(), "its row");
    assert_eq!(super::said(&shell, cx), "example.org");
    let profile = cx.read(|cx| shell.read(cx).workspace.loose()[0].project.clone());
    assert_eq!(profile, std::path::Path::new(crate::browser::NO_PROJECT));
    open_page(window, &shell, Some("https://example.org/next"), cx);
    assert_eq!(
        loose(&shell, cx),
        ["https://example.org/inbox", "https://example.org/next"]
    );

    act(window, PreviousSpace, cx);
    assert_eq!(project(cx), Some(roots[0].clone()));
    assert!(loose(&shell, cx).is_empty());
    assert_eq!(shown(&shell, cx), (None, false), "the chat");
    assert!(drawn(window, "arc-loose-0", cx).is_none());
    open_page(window, &shell, Some("https://example.org/work"), cx);
    assert!(loose(&shell, cx).is_empty());
    assert_eq!(listed(&shell, cx), ["A chat", "example.org/work"]);
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    draw(window, cx);
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.org/next".into()), true)
    );

    click(window, "arc-loose-0", cx);
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.org/inbox".into()), true)
    );
    click(window, "arc-loose-1", cx);
    act(window, CloseTab, cx);
    assert_eq!(loose(&shell, cx), ["https://example.org/inbox"]);
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.org/inbox".into()), true)
    );
    click(window, "arc-loose-close-0", cx);
    assert!(loose(&shell, cx).is_empty());
    assert_eq!(shown(&shell, cx), (None, false));
    assert_eq!(cx.read(|cx| settings::get(cx).space), 1);
}

#[gpui_kit::test]
fn a_pages_tab_moves_between_a_worktree_the_loose_pages_and_the_pinned_by_dragging(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = arc_shell(&["arc-loose-drag"], cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    let pages = [
        "https://a.example/",
        "https://b.example/",
        "https://c.example/",
    ];
    for page in pages {
        open_page(window, &shell, Some(page), cx);
    }
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    draw(window, cx);
    assert!(
        drawn(window, "arc-loose-strip", cx).is_none(),
        "no list, with none"
    );

    drag(window, "tab-0-0-1", "tab-0-0-1", cx);
    assert!(drawn(window, "arc-loose-strip", cx).is_none());
    take_up(window, "tab-0-0-0", cx);
    assert!(drawn(window, "arc-loose-strip", cx).is_none());
    let_go_on(window, "tab-0-0-0", cx);
    take_up(window, "tab-0-0-1", cx);
    assert!(
        drawn(window, "arc-loose-strip", cx).is_some(),
        "the strip, for the page in hand"
    );
    let_go_on(window, "arc-loose-strip", cx);
    assert_eq!(loose(&shell, cx), [pages[0]]);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), true));
    assert_eq!(listed(&shell, cx), ["A chat", "b.example", "c.example"]);
    assert!(drawn(window, "arc-loose-strip", cx).is_none());

    drag_onto(window, "tab-0-0-1", "arc-loose-0", cx);
    assert_eq!(loose(&shell, cx), [pages[1], pages[0]]);
    drag_onto(window, "tab-0-0-0", "arc-loose-0", cx);
    assert_eq!(loose(&shell, cx).len(), 2);
    assert_eq!(listed(&shell, cx), ["A chat", "c.example"]);

    let none = Modifiers::default();
    let from = drawn(window, "arc-loose-0", cx)
        .expect("the first row")
        .center();
    let below = drawn(window, "arc-loose-1", cx).expect("the second row");
    let past = below.center() + point(px(0.), px(8.));
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_down(from, MouseButton::Left, none);
    vcx.simulate_mouse_move(from + point(px(0.), px(3.)), Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_move(past, Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_up(past, MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(loose(&shell, cx), [pages[0], pages[1]]);

    drag_onto(window, "arc-loose-0", "tab-0-0-1", cx);
    assert_eq!(loose(&shell, cx), [pages[1]]);
    assert_eq!(listed(&shell, cx), ["A chat", "a.example", "c.example"]);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), false));

    drag_onto(window, "arc-loose-0", "arc-pinned", cx);
    let pinned = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.pinned().len());
    assert_eq!((pinned(cx), loose(&shell, cx).len()), (1, 0));
    drag_onto(window, "arc-pinned-0", "arc-loose-strip", cx);
    assert_eq!(
        (pinned(cx), loose(&shell, cx)),
        (0, vec![pages[1].to_owned()])
    );
    assert_eq!(shown(&shell, cx), (Some(pages[1].into()), true));
}

#[gpui_kit::test]
fn a_loose_page_closed_comes_back_where_it_was_with_cmd_shift_t(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-loose-reopen"], cx);
    act(window, NewSpace, cx);
    let page = |name: &str| format!("https://example.org/{name}");
    for name in ["one", "two", "three"] {
        open_page(window, &shell, Some(&page(name)), cx);
    }
    click(window, "arc-loose-1", cx);
    act(window, CloseTab, cx);
    assert_eq!(loose(&shell, cx), [page("one"), page("three")]);
    let reopened = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.reopen_closed_checkout_tab(window, cx))
    });
    assert_eq!(reopened.unwrap(), None);
    act(window, PreviousSpace, cx);
    act(window, ReopenClosedTab, cx);
    draw(window, cx);
    assert_eq!(cx.read(|cx| settings::get(cx).space), 1);
    assert_eq!(loose(&shell, cx), [page("one"), page("two"), page("three")]);
    assert_eq!(shown(&shell, cx), (Some(page("two")), true));
}

#[gpui_kit::test]
fn a_pinned_or_a_loose_page_is_copied_into_the_selected_worktree_from_its_menu(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = arc_shell(&["arc-copy-to-worktree"], cx);
    let pinned = open_page(window, &shell, Some("https://mail.example/inbox"), cx);
    let page = open_page(window, &shell, Some("https://bank.example/"), cx);
    shell.update(cx, |shell, cx| {
        assert!(shell.pin_tab_of(pinned, usize::MAX, cx));
        shell.loosen_tab_of(page, usize::MAX, cx);
    });
    draw(window, cx);
    let tabs = |cx: &TestAppContext| {
        cx.read(|cx| {
            let unit = &shell.read(cx).workspace.projects[0].branches[0];
            (unit.tabs.iter())
                .map(|tab| tab.first().address().unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        })
    };
    let menu = |cx: &TestAppContext| cx.read(|cx| PinnedMenu::of(shell.read(cx), pinned));
    assert!(tabs(cx).is_empty());
    assert!(menu(cx).worktree, "a worktree is selected to copy into");

    act(window, CopyToWorktree(pinned), cx);
    assert_eq!(tabs(cx), ["https://mail.example/inbox"]);
    let copy = cx.read(|cx| shell.read(cx).shown_tab_key());
    assert!(copy.is_some_and(|copy| copy != pinned), "the copy shows");
    act(window, CopyToWorktree(page), cx);
    assert_eq!(tabs(cx), ["https://mail.example/inbox", "https://bank.example/"]);
    assert_eq!(loose(&shell, cx), ["https://bank.example/"]);
    let still = cx.read(|cx| shell.read(cx).workspace.pinned().len());
    assert_eq!(still, 1);

    act(window, NewSpace, cx);
    assert!(!menu(cx).worktree, "greyed out");
    act(window, CopyToWorktree(pinned), cx);
    assert_eq!(tabs(cx).len(), 2);
}

#[gpui_kit::test]
fn shared_loose_pages_are_the_same_in_every_space_and_each_goes_back_to_its_own(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = arc_shell(&["arc-loose-shared"], cx);
    let (one, two) = ("https://one.example/", "https://two.example/");
    act(window, NewSpace, cx);
    open_page(window, &shell, Some(one), cx);
    act(window, PreviousSpace, cx);
    assert!(loose(&shell, cx).is_empty(), "each space's own, at first");
    let space = |cx: &TestAppContext| cx.read(|cx| settings::get(cx).space);
    let set = |pages: LoosePages, cx: &mut TestAppContext| {
        cx.update(|cx| settings::update(cx, |settings| settings.loose_pages = pages));
        draw(window, cx);
    };

    set(LoosePages::Shared, cx);
    assert_eq!(loose(&shell, cx), [one]);
    assert!(drawn(window, "arc-loose-0", cx).is_some(), "its row");
    click(window, "arc-loose-0", cx);
    assert_eq!(shown(&shell, cx), (Some(one.into()), true));
    assert_eq!(space(cx), 0, "shown where it's listed");
    open_page(window, &shell, Some(two), cx);
    assert_eq!(loose(&shell, cx), [one, two]);
    act(window, NextSpace, cx);
    assert_eq!(loose(&shell, cx), [one, two], "the same in the other space");

    set(LoosePages::PerSpace, cx);
    assert_eq!(loose(&shell, cx), [one]);
    act(window, PreviousSpace, cx);
    assert_eq!(
        loose(&shell, cx),
        [two],
        "opened while the first space was shown"
    );
}
