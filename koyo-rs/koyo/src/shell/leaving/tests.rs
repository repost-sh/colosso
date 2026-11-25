//! A page the person closes is asked first, as Chrome asks it: one that asks to confirm leaving
//! asks the person, its tab staying until it may be left, and staying when they stay. Each kind of
//! tab goes then as it goes when nothing asks: a pinned page stays pinned, a loose one is kept for
//! ⌘⇧T, a worktree's tab is gone.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_app_control::ops::browser::{DialogAction, DialogKind};
use koyo_browser_ipc::{AppRequest, Dialog, HostEvent, PaneId};

use crate::browser::agent::{Actor, Unanswered};
use crate::browser::{BrowserView, FakeHost};
use crate::model::{Pane, Spot, Tab};
use crate::projects::RemoveProject;
use crate::shell::{ClosePinned, CloseTab, Shell};
use crate::test_support::{act, draw, open_page_view, pin, pinned, press, shell_on};
use crate::modal;

/// The person may leave `pane`'s page, as the host says, and the window draws what that changes.
fn may_leave(host: &FakeHost, pane: PaneId, window: AnyWindowHandle, cx: &mut TestAppContext) {
    host.says(HostEvent::LeaveAnswered { pane, may: true }, cx);
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
    let question = Dialog::BeforeUnload { reload: false, closing: true };
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
    let (window, shell, _) = shell_on("leave-pinned", cx);
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
    assert_eq!(page_in(&shell, key, cx), Some(view), "its tab stays till the page may be left");

    may_leave(&host, pane, window, cx);
    assert_eq!(pinned(&shell, cx), ["https://a.example/"], "still pinned");
    assert_eq!(page_in(&shell, key, cx), None, "its page closed");
}

#[gpui_kit::test]
fn a_pinned_page_closed_from_its_menu_out_of_sight_shows_as_it_asks(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-pinned-unseen", cx);
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
    may_leave(&host, pane, window, cx);
    assert_eq!(page_in(&shell, keys[0], cx), None, "its page closed, and it stays pinned");
    assert_eq!(pinned(&shell, cx).len(), 2);
}

#[gpui_kit::test]
fn a_loose_page_the_person_closes_is_kept_to_open_again_once_it_went(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-loose", cx);
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
    may_leave(&host, pane, window, cx);
    assert_eq!(loose(cx), 0, "it went, its tab with it");
    assert!(matches!(reopened(cx), Some(Spot::Loose(_))), "kept for ⌘⇧T");
}

#[gpui_kit::test]
fn a_worktrees_page_tab_the_person_closes_goes_once_its_page_went(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-tab", cx);
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
    may_leave(&host, pane, window, cx);
    assert_eq!(tabs(cx), Some(0), "a page that doesn't ask goes as it closes");
}

#[gpui_kit::test]
fn a_page_whose_question_is_withdrawn_is_not_being_closed_any_more(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-withdrawn", cx);
    let host = FakeHost::serve("leave-withdrawn-host", cx);
    let view = open_page_view(window, &shell, "https://a.example/", cx);
    let pane = host_pane(&view, cx);
    act(window, CloseTab, cx);
    asks_to_leave(&host, (pane, 1), window, cx);
    host.says(HostEvent::DialogClosed { pane, dialog: 1 }, cx);
    host.says(HostEvent::LeaveAnswered { pane, may: false }, cx);
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
fn a_page_split_beside_another_asks_as_its_pane_closes(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-split", cx);
    let host = FakeHost::serve("leave-split-host", cx);
    let [_, b] = two_pages(window, &shell, ("https://a.example/", "https://b.example/"), cx);
    let pane_b = host_pane(&b, cx);
    let id = shell.read_with(cx, |shell, _| shell.pane_showing(&b)).expect("b's pane");
    host.asked(cx);
    let panes = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.tab_shown()).map_or(0, |tab| tab.panes().len()))
    };

    (cx.update_window(window, |_, window, cx| shell.update(cx, |shell, cx| shell.close_pane(id, window, cx))))
        .unwrap();
    draw(window, cx);
    assert_eq!(asked_of(&host, pane_b, cx), [AppRequest::Leave { pane: pane_b }]);
    assert_eq!(panes(cx), 2, "its pane stays until it may be left");
    may_leave(&host, pane_b, window, cx);
    assert_eq!(panes(cx), 1, "its pane goes");
}

/// What the app asked of each of `pages`, in order: whether the person may leave it, and that it
/// closes.
fn asked_of_each(host: &FakeHost, cx: &mut TestAppContext) -> Vec<AppRequest> {
    (host.asked(cx).into_iter())
        .filter(|request| matches!(request, AppRequest::Leave { .. } | AppRequest::Close { .. }))
        .collect()
}

/// A close that takes two pages, `close`d by the person, asks the first, then the second; the
/// person staying on the second, both stay, and leaving both, what was closing goes, as `gone`
/// says.
fn asks_each(
    name: &str,
    close: impl Fn(&Entity<Shell>, AnyWindowHandle, &mut TestAppContext),
    gone: impl Fn(&Entity<Shell>, &TestAppContext) -> bool,
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = shell_on(name, cx);
    let host = FakeHost::serve(&format!("{name}-host"), cx);
    let [a, b] = two_pages(window, &shell, ("https://a.example/", "https://b.example/"), cx);
    let [a, b] = [host_pane(&a, cx), host_pane(&b, cx)];
    host.asked(cx);

    close(&shell, window, cx);
    draw(window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: a }]);
    may_leave(&host, a, window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: b }], "then the other");
    asks_to_leave(&host, (b, 1), window, cx);
    in_dialog("escape", cx);
    host.says(HostEvent::LeaveAnswered { pane: b, may: false }, cx);
    assert!(!gone(&shell, cx), "the person stayed on the second");
    assert_eq!(asked_of_each(&host, cx), [], "neither closes");

    close(&shell, window, cx);
    draw(window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: a }], "both are asked again");
    may_leave(&host, a, window, cx);
    may_leave(&host, b, window, cx);
    assert!(gone(&shell, cx), "the person may leave both");
}

/// Whether the selected checkout has no tab.
fn no_tabs(shell: &Entity<Shell>, cx: &TestAppContext) -> bool {
    cx.read(|cx| shell.read(cx).workspace.branch().is_none_or(|unit| unit.tabs.is_empty()))
}

#[gpui_kit::test]
fn a_tab_of_two_pages_asks_each_before_it_closes(cx: &mut TestAppContext) {
    asks_each("leave-two", |_, window, cx| act(window, CloseTab, cx), no_tabs, cx);
}

#[gpui_kit::test]
fn removing_a_project_asks_each_of_its_pages_first(cx: &mut TestAppContext) {
    let remove = |shell: &Entity<Shell>, window: AnyWindowHandle, cx: &mut TestAppContext| {
        let project = cx.read(|cx| shell.read(cx).workspace.project().expect("a project").path.clone());
        act(window, RemoveProject(project), cx);
    };
    let gone = |shell: &Entity<Shell>, cx: &TestAppContext| cx.read(|cx| shell.read(cx).workspace.projects.is_empty());
    asks_each("leave-project", remove, gone, cx);
}

#[gpui_kit::test]
fn closing_the_window_asks_each_page_first(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-window", cx);
    let host = FakeHost::serve("leave-window-host", cx);
    let pane = host_pane(&open_page_view(window, &shell, "https://a.example/", cx), cx);
    host.asked(cx);
    let closes = |cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| shell.update(cx, |shell, cx| shell.ask_to_close(window, cx)))).unwrap();
    };

    closes(cx);
    assert!(cx.windows().contains(&window), "it waits for the page");
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane }]);
    asks_to_leave(&host, (pane, 1), window, cx);
    in_dialog("escape", cx);
    assert!(cx.windows().contains(&window), "the person stayed");

    closes(cx);
    host.says(HostEvent::LeaveAnswered { pane, may: true }, cx);
    cx.run_until_parked();
    assert!(!cx.windows().contains(&window), "the window closes");
}

/// Quits as ⌘Q does, up to what it does once it may, which is kept in what's given back.
fn quits(shell: &Entity<Shell>, window: AnyWindowHandle, cx: &mut TestAppContext) -> Rc<Cell<bool>> {
    let quit = Rc::new(Cell::new(false));
    let done = quit.clone();
    (cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.ask_all_then(move |_| done.set(true), window, cx));
    }))
    .unwrap();
    draw(window, cx);
    quit
}

#[gpui_kit::test]
fn quitting_again_while_a_page_asks_waits_for_the_answer(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-quit", cx);
    let host = FakeHost::serve("leave-quit-host", cx);
    let view = open_page_view(window, &shell, "https://a.example/", cx);
    let pane = host_pane(&view, cx);
    host.asked(cx);
    let quit = quits(&shell, window, cx);
    asks_to_leave(&host, (pane, 1), window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane }]);

    let again = quits(&shell, window, cx);
    act(window, CloseTab, cx);
    assert_eq!(asked_of_each(&host, cx), [], "the page isn't asked again");
    assert_eq!((quit.get(), again.get()), (false, false));
    let agent = Actor { id: "a".into(), label: "Agent".into() };
    let answered = view.update(cx, |view, cx| view.agent_answers(&agent, DialogAction::Accept, None, cx));
    assert_eq!(answered, Err(Unanswered::PersonAnswers(DialogKind::Leave)), "nor by an agent");

    in_dialog("enter", cx);
    may_leave(&host, pane, window, cx);
    assert_eq!((quit.get(), again.get()), (true, false), "the first quits");
}

#[gpui_kit::test]
fn a_page_gone_as_the_person_quits_holds_nothing_up(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-gone", cx);
    let host = FakeHost::serve("leave-gone-host", cx);
    let [a, b] = two_pages(window, &shell, ("https://a.example/", "https://b.example/"), cx);
    let (first, second) = (host_pane(&a, cx), host_pane(&b, cx));
    let id = shell.read_with(cx, |shell, _| shell.pane_showing(&a)).expect("a's pane");
    drop((a, b));
    host.asked(cx);
    let quit = quits(&shell, window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: first }]);

    shell.update(cx, |shell, cx| shell.close_pane_now(id, cx));
    draw(window, cx);
    let asked = asked_of_each(&host, cx);
    assert!(asked.contains(&AppRequest::Leave { pane: second }), "an agent closed it: the next is asked, {asked:?}");
    may_leave(&host, second, window, cx);
    assert!(quit.get());
}

#[gpui_kit::test]
fn a_page_that_asks_for_another_reason_is_shown_and_not_left(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-already", cx);
    let host = FakeHost::serve("leave-already-host", cx);
    let [a, b] = two_pages(window, &shell, ("https://a.example/", "https://b.example/"), cx);
    let (first, second) = (host_pane(&a, cx), host_pane(&b, cx));
    let question = Dialog::BeforeUnload { reload: false, closing: false };
    host.says(HostEvent::Dialog { pane: second, dialog: 1, origin: "https://b.example".into(), question }, cx);
    draw(window, cx);
    host.asked(cx);
    let quit = quits(&shell, window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: first }]);
    may_leave(&host, first, window, cx);
    assert_eq!(asked_of_each(&host, cx), [], "a link of the second's asks already");
    in_dialog("enter", cx);
    host.says(HostEvent::Closed { pane: second }, cx);
    draw(window, cx);
    assert!(!quit.get(), "the person followed the link, and nothing more");

    let quit = quits(&shell, window, cx);
    assert_eq!(asked_of_each(&host, cx), [AppRequest::Leave { pane: first }]);
    host.says(HostEvent::LeaveAnswered { pane: first, may: false }, cx);
    may_leave(&host, first, window, cx);
    assert!(!quit.get(), "the page asks for another reason, which the app hadn't heard");
}

#[gpui_kit::test]
fn a_page_asked_for_two_closes_is_asked_for_the_later_alone(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("leave-twice", cx);
    let host = FakeHost::serve("leave-twice-host", cx);
    let view = open_page_view(window, &shell, "https://a.example/", cx);
    let pane = host_pane(&view, cx);
    host.asked(cx);
    act(window, CloseTab, cx);
    let quit = quits(&shell, window, cx);
    may_leave(&host, pane, window, cx);
    assert!(quit.get(), "quitting takes the close's place");
    assert_eq!(page_in(&shell, shell.read_with(cx, |shell, _| shell.pane_showing(&view)).expect("its pane"), cx), Some(view));
    may_leave(&host, pane, window, cx);
    let tabs = cx.read(|cx| shell.read(cx).workspace.branch().map(|unit| unit.tabs.len()));
    assert_eq!(tabs, Some(1), "and a later answer does nothing");
}
