//! A page the person closes is asked first, as Chrome asks it: one that asks to confirm leaving
//! asks the person, its tab staying until it went, and staying when they stay. Each kind of tab
//! goes then as it goes today: a pinned page stays pinned, a loose one is kept for ⌘⇧T, a
//! worktree's tab is gone.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_browser_ipc::{AppRequest, Dialog, HostEvent, PaneId};

use crate::browser::{BrowserView, FakeHost};
use crate::files::tests::support::press;
use crate::model::{Pane, Spot, Tab};
use crate::settings::Layout;
use crate::shell::{ClosePinned, CloseTab, Shell};
use crate::test_support::{act, draw, open_page_view, pin, pinned, shell_on};
use crate::modal;

/// `pane`'s page goes, as the host says it does, and the window draws what that changes.
fn goes(host: &FakeHost, pane: PaneId, window: AnyWindowHandle, cx: &mut TestAppContext) {
    host.says(HostEvent::TabClosed { pane }, cx);
    host.says(HostEvent::Closed { pane }, cx);
    draw(window, cx);
}

/// The page pane `pane` shows, while it shows one.
fn page_in(shell: &Entity<Shell>, pane: u64, cx: &TestAppContext) -> Option<Entity<BrowserView>> {
    cx.read(|cx| shell.read(cx).workspace.pane(pane)?.view().cloned())
}

fn host_pane(view: &Entity<BrowserView>, cx: &TestAppContext) -> PaneId {
    cx.read(|cx| view.read(cx).host_pane())
}

/// What the app asked of `host` about `pane`'s page since it was last read: whether to leave it
/// and whether to close it, and the answers to its questions.
fn asked_of(host: &FakeHost, pane: PaneId, cx: &mut TestAppContext) -> Vec<AppRequest> {
    let asked = host.asked(cx);
    asked
        .into_iter()
        .filter(|request| {
            matches!(request, AppRequest::Leave { .. } | AppRequest::Close { .. } | AppRequest::Answer { .. })
                && request.pane() == Some(pane)
        })
        .collect()
}

/// `pane`'s page asks the person to confirm leaving it, as its question `dialog`, and the window
/// draws what that shows.
fn asks_to_leave(host: &FakeHost, (pane, dialog): (PaneId, u64), window: AnyWindowHandle, cx: &mut TestAppContext) {
    let question = Dialog::BeforeUnload { reload: false };
    let origin = "https://a.example".into();
    host.says(HostEvent::Dialog { pane, dialog, origin, question }, cx);
    draw(window, cx);
}

/// Presses `key` in the dialog on screen, as the person answers it: `enter` leaves, `escape`
/// stays.
fn in_dialog(key: &str, cx: &mut TestAppContext) {
    let window = cx.update(|cx| modal::window(cx)).expect("the page's question is on screen");
    press(window, key, cx);
}

#[gpui_kit::test]
fn a_pinned_page_the_person_closes_asks_first_and_stays_pinned_once_it_went(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-pinned", Layout::Arc, cx);
    let host = FakeHost::serve("leave-pinned-host", cx);
    let [key] = pin(window, &shell, &["https://a.example/"], cx)[..] else {
        panic!("one pinned page");
    };
    draw(window, cx);
    let view = page_in(&shell, key, cx).expect("the pinned page, open");
    let pane = host_pane(&view, cx);
    host.asked(cx);

    act(window, CloseTab, cx);
    act(window, CloseTab, cx);
    let leave = AppRequest::Leave { pane };
    assert_eq!(asked_of(&host, pane, cx), [leave.clone(), leave], "asked at each close");
    assert_eq!(page_in(&shell, key, cx), Some(view.clone()), "open until it went");

    asks_to_leave(&host, (pane, 1), window, cx);
    in_dialog("escape", cx);
    let stayed = AppRequest::Answer { pane, dialog: 1, answer: koyo_browser_ipc::Answer::Dismiss };
    assert_eq!(asked_of(&host, pane, cx), [stayed]);
    assert_eq!(page_in(&shell, key, cx), Some(view.clone()), "the person stayed");

    act(window, CloseTab, cx);
    assert_eq!(asked_of(&host, pane, cx), [AppRequest::Leave { pane }], "asked again");
    asks_to_leave(&host, (pane, 2), window, cx);
    in_dialog("enter", cx);
    let left = AppRequest::Answer { pane, dialog: 2, answer: koyo_browser_ipc::Answer::Accept };
    assert_eq!(asked_of(&host, pane, cx), [left]);
    assert_eq!(page_in(&shell, key, cx), Some(view), "the page goes, then its tab");

    goes(&host, pane, window, cx);
    assert_eq!(pinned(&shell, cx), ["https://a.example/"], "still pinned");
    assert_eq!(page_in(&shell, key, cx), None, "its page closed");
}

#[gpui_kit::test]
fn a_pinned_page_closed_from_its_menu_out_of_sight_shows_as_it_asks(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-pinned-unseen", Layout::Arc, cx);
    let host = FakeHost::serve("leave-pinned-unseen-host", cx);
    let keys = pin(window, &shell, &["https://a.example/", "https://b.example/"], cx);
    shell.update(cx, |shell, cx| assert!(shell.show_tab_holding(keys[0], cx)));
    draw(window, cx);
    let view = page_in(&shell, keys[0], cx).expect("the first pinned page, open");
    let pane = host_pane(&view, cx);
    shell.update(cx, |shell, cx| assert!(shell.show_tab_holding(keys[1], cx)));
    draw(window, cx);
    host.asked(cx);

    act(window, ClosePinned(keys[0]), cx);
    assert_eq!(asked_of(&host, pane, cx), [AppRequest::Leave { pane }]);
    let shown = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.apart_shown_key());
    assert_eq!(shown(cx), Some(keys[1]), "the page shown stays shown while none asks");

    asks_to_leave(&host, (pane, 1), window, cx);
    assert_eq!(shown(cx), Some(keys[0]), "the page asking shows, its question with it");
    in_dialog("enter", cx);
    goes(&host, pane, window, cx);
    assert_eq!(page_in(&shell, keys[0], cx), None, "its page closed, and it stays pinned");
    assert_eq!(pinned(&shell, cx).len(), 2);
}

#[gpui_kit::test]
fn a_loose_page_the_person_closes_is_kept_to_open_again_once_it_went(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-loose", Layout::Arc, cx);
    let host = FakeHost::serve("leave-loose-host", cx);
    let key = (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_page(Some("https://a.example/".into()), (true, false), None, window, cx)
        })
    }))
    .unwrap()
    .expect("a loose page");
    draw(window, cx);
    let view = page_in(&shell, key, cx).expect("the loose page, open");
    let pane = host_pane(&view, cx);
    host.asked(cx);
    let loose = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.loose().len());
    let reopened = |cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            shell.update(cx, |shell, cx| shell.reopen_closed_tab(window, cx))
        }))
        .unwrap()
    };

    act(window, CloseTab, cx);
    assert_eq!(asked_of(&host, pane, cx), [AppRequest::Leave { pane }]);
    assert_eq!(loose(cx), 1, "its tab stays until it went");
    assert_eq!(reopened(cx), None, "nothing closed to open again");

    asks_to_leave(&host, (pane, 1), window, cx);
    in_dialog("escape", cx);
    assert_eq!(loose(cx), 1, "the person stayed");

    act(window, CloseTab, cx);
    asks_to_leave(&host, (pane, 2), window, cx);
    in_dialog("enter", cx);
    assert_eq!(loose(cx), 1);
    goes(&host, pane, window, cx);
    assert_eq!(loose(cx), 0, "it went, its tab with it");
    assert!(matches!(reopened(cx), Some(Spot::Loose(_))), "kept for ⌘⇧T");
}

#[gpui_kit::test]
fn a_worktrees_page_tab_the_person_closes_goes_once_its_page_went(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-tab", Layout::Orca, cx);
    let host = FakeHost::serve("leave-tab-host", cx);
    let view = open_page_view(window, &shell, "https://a.example/", cx);
    let pane = host_pane(&view, cx);
    host.asked(cx);
    let tabs = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));

    act(window, CloseTab, cx);
    assert_eq!(asked_of(&host, pane, cx), [AppRequest::Leave { pane }]);
    assert_eq!(tabs(cx), Some(1), "its tab stays until it went");
    asks_to_leave(&host, (pane, 1), window, cx);
    in_dialog("escape", cx);
    let stayed = AppRequest::Answer { pane, dialog: 1, answer: koyo_browser_ipc::Answer::Dismiss };
    assert_eq!(asked_of(&host, pane, cx), [stayed]);
    assert_eq!(tabs(cx), Some(1), "the person stayed");

    act(window, CloseTab, cx);
    assert_eq!(asked_of(&host, pane, cx), [AppRequest::Leave { pane }]);
    goes(&host, pane, window, cx);
    assert_eq!(tabs(cx), Some(0), "a page that doesn't ask goes as it closes");
}

#[gpui_kit::test]
fn a_page_whose_question_is_withdrawn_is_not_being_closed_any_more(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-withdrawn", Layout::Orca, cx);
    let host = FakeHost::serve("leave-withdrawn-host", cx);
    let view = open_page_view(window, &shell, "https://a.example/", cx);
    let pane = host_pane(&view, cx);
    act(window, CloseTab, cx);
    asks_to_leave(&host, (pane, 1), window, cx);
    host.says(HostEvent::DialogClosed { pane, dialog: 1 }, cx);
    host.says(HostEvent::Closed { pane }, cx);
    draw(window, cx);
    let tabs = cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    assert_eq!(tabs, Some(1), "the person never said they'd leave");
}

/// A tab of the selected checkout with a page at `a` beside a page at `b`, shown and drawn: its
/// two pages.
fn two_pages(window: AnyWindowHandle, shell: &Entity<Shell>, (a, b): (&str, &str), cx: &mut TestAppContext) -> [Entity<BrowserView>; 2] {
    let pages = cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            let project = shell.workspace.project().expect("a project").path.clone();
            let pages = [a, b].map(|url| shell.browser_view(url.into(), &project, window, cx));
            let [first, second] = pages.clone().map(|page| Tab::Single(Pane::browser(page, cx)));
            shell.workspace.push_tab(Tab::beside(first, second));
            cx.notify();
            pages
        })
    });
    draw(window, cx);
    pages.unwrap()
}

#[gpui_kit::test]
fn a_page_split_beside_another_asks_as_its_pane_closes_and_a_tab_of_two_goes_unasked(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-split", Layout::Orca, cx);
    let host = FakeHost::serve("leave-split-host", cx);
    let [a, b] = two_pages(window, &shell, ("https://a.example/", "https://b.example/"), cx);
    let pane_b = host_pane(&b, cx);
    drop(a);
    let id = shell.read_with(cx, |shell, _| shell.pane_showing(&b)).expect("b's pane");
    host.asked(cx);
    let panes = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.tab_shown()).map_or(0, |tab| tab.panes().len()))
    };

    (cx.update_window(window, |_, window, cx| shell.update(cx, |shell, cx| shell.close_pane(id, window, cx))))
        .unwrap();
    draw(window, cx);
    assert_eq!(asked_of(&host, pane_b, cx), [AppRequest::Leave { pane: pane_b }]);
    assert_eq!(panes(cx), 2, "its pane stays until it went");
    goes(&host, pane_b, window, cx);
    assert_eq!(panes(cx), 1, "it went, its pane with it");

    drop(two_pages(window, &shell, ("https://c.example/", "https://d.example/"), cx));
    host.asked(cx);
    act(window, CloseTab, cx);
    let tabs = cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    assert_eq!(tabs, Some(1), "a tab of two pages goes at once");
    let asked = host.asked(cx);
    let leave = |request: &AppRequest| matches!(request, AppRequest::Leave { .. });
    assert!(!asked.iter().any(leave), "neither is asked: {asked:?}");
}
