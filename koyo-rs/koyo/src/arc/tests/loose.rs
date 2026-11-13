//! The loose pages of a space (`docs/arc-sidebar.md` 11): a page opened where no project is
//! shown is one, and so is one opened from a pinned or a loose page; each space lists its own;
//! a page's tab let go on them is one, a loose page let go on a worktree's tabs is its tab, and
//! on the pinned pages is pinned; and a space with no project shows no dot.

use gpui_kit::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

use super::{act, arc_shell, chat, drag, drag_onto, drawn, let_go_on, listed, take_up};
use crate::arc::actions::{NewSpace, PreviousSpace};
use crate::files::tests::{dialog, press};
use crate::launcher::{Launcher, NewTab};
use crate::settings;
use crate::shell::{CloseTab, Shell};
use crate::test_support::{click, draw, open_page};

/// The loose pages of the space shown, by the addresses they show, in the order of their rows.
fn loose(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        (shell.read(cx).workspace.loose().iter())
            .map(|page| page.tab.first().address(cx).unwrap_or_default())
            .collect()
    })
}

/// The address of the page the panes show, and whether it's a loose one.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<String>, bool) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address(cx));
        (address, workspace.loose_shown().is_some())
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
    // A space with no project: it shows none, and no dot where its name would be.
    act(window, NewSpace, cx);
    let project = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.project()).map(|project| project.path.clone()))
    };
    assert_eq!(project(cx), None);
    assert!(drawn(window, "arc-project-name", cx).is_none());

    // ⌘T opens the launcher there all the same, for a page: Tab goes to no other kind, and the
    // address typed opens as a loose page of the space, shown.
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
    let said = cx.read(|cx| crate::arc::top::said(shell.read(cx), cx).0.to_string());
    assert_eq!(said, "example.org");
    let profile = cx.read(|cx| shell.read(cx).workspace.loose()[0].project.clone());
    assert_eq!(profile, std::path::Path::new(crate::browser::NO_PROJECT));
    // Another, opened from it, comes right after it and shows.
    open_page(window, &shell, Some("https://example.org/next"), cx);
    assert_eq!(
        loose(&shell, cx),
        ["https://example.org/inbox", "https://example.org/next"]
    );

    // The first space lists none of them, and shows its project's tab; back, they're there, the
    // one that was shown shown.
    act(window, PreviousSpace, cx);
    assert_eq!(project(cx), Some(roots[0].clone()));
    assert!(loose(&shell, cx).is_empty());
    assert_eq!(shown(&shell, cx), (None, false), "the chat");
    assert!(drawn(window, "arc-loose-0", cx).is_none());
    // Where a project's tab is shown, a page opened is the worktree's.
    open_page(window, &shell, Some("https://example.org/work"), cx);
    assert!(loose(&shell, cx).is_empty());
    assert_eq!(listed(&shell, cx), ["A chat", "example.org/work"]);
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    draw(window, cx);
    assert_eq!(
        shown(&shell, cx),
        (Some("https://example.org/next".into()), true)
    );

    // A row picked shows its page; ⌘W closes the one shown, and the one before it shows; its ×
    // closes the last, and nothing is shown.
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
    // Back on the chat, so no page's tab is the one shown.
    shell.update(cx, |shell, cx| shell.pick_tab(0, 0, 0, cx));
    draw(window, cx);
    assert!(
        drawn(window, "arc-loose-strip", cx).is_none(),
        "no list, with none"
    );

    // A page's tab moved along its own list brings no strip, nor does a chat's tab taken off
    // it; a page's taken off it does, and let go on it is the space's first loose page, shown.
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

    // Another, let go on its row, takes its place; the chat's tab let go there stays a tab.
    drag_onto(window, "tab-0-0-1", "arc-loose-0", cx);
    assert_eq!(loose(&shell, cx), [pages[1], pages[0]]);
    drag_onto(window, "tab-0-0-0", "arc-loose-0", cx);
    assert_eq!(loose(&shell, cx).len(), 2);
    assert_eq!(listed(&shell, cx), ["A chat", "c.example"]);

    // A row dragged past the other's middle moves after it.
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

    // Onto a worktree's tab, it's a tab of the worktree where that tab is, and shows there.
    drag_onto(window, "arc-loose-0", "tab-0-0-1", cx);
    assert_eq!(loose(&shell, cx), [pages[1]]);
    assert_eq!(listed(&shell, cx), ["A chat", "a.example", "c.example"]);
    assert_eq!(shown(&shell, cx), (Some(pages[0].into()), false));

    // Onto the pinned pages' zone, it's pinned; and its tile, let go on a loose page's row or
    // on the strip, is loose again.
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
