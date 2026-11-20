//! A file chooser a page opens while an agent holds it is the agent's, kept from the person's
//! screen, until the agent answers it or loses the page; the person's own goes to the
//! system's panel.

use std::path::Path;

use gpui_kit::{AppContext as _, Entity, TestAppContext, px, size};
use koyo_app_control::ops::browser::{DialogAction, DialogKind};
use koyo_browser_ipc::{Answer, AppRequest, Dialog, FileMode, HostEvent, PaneId};

use crate::browser::agent::{Actor, Unanswered};
use crate::browser::control::{Leased as _, Mode};
use crate::browser::host::fake::FakeHost;
use crate::browser::view::BrowserView;
use crate::test_support::{draw, in_window};

fn agent() -> Actor {
    Actor {
        id: "a".into(),
        label: "Agent".into(),
    }
}

/// A page of the fake host, drawn, which an agent holds when `held`.
fn page(
    name: &str,
    held: bool,
    cx: &mut TestAppContext,
) -> (FakeHost, Entity<BrowserView>, PaneId) {
    cx.update(crate::test_support::init);
    let host = FakeHost::serve(name, cx);
    let (window, view) = in_window(size(px(800.), px(600.)), cx, |window, cx| {
        cx.new(|cx| {
            BrowserView::new(
                "https://a.example/".into(),
                Path::new("/work/app"),
                window,
                cx,
            )
        })
    });
    draw(window, cx);
    if held {
        view.update(cx, |view, cx| view.agent_claims(agent(), false, None, cx))
            .expect("free");
    }
    let pane = view.read_with(cx, |view, _| view.pane);
    host.asked(cx);
    (host, view, pane)
}

/// The page asks for files as question `id`.
fn asks_files(host: &FakeHost, pane: PaneId, id: u64, mode: FileMode, cx: &mut TestAppContext) {
    let question = Dialog::File {
        mode,
        title: String::new(),
        default_path: String::new(),
        accept: vec!["image/*".into()],
    };
    host.says(
        HostEvent::Dialog {
            pane,
            dialog: id,
            origin: "https://a.example".into(),
            question,
        },
        cx,
    );
}

/// The answers the host was given.
fn answers(host: &FakeHost, cx: &mut TestAppContext) -> Vec<(u64, Answer)> {
    (host.asked(cx).into_iter())
        .filter_map(|asked| match asked {
            AppRequest::Answer { dialog, answer, .. } => Some((dialog, answer)),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn a_chooser_an_agents_act_opens_is_the_agents_and_takes_its_files(cx: &mut TestAppContext) {
    let (host, view, pane) = page("chooser-agents", true, cx);
    asks_files(&host, pane, 7, FileMode::OpenMultiple, cx);
    let asked = view
        .read_with(cx, |view, _| view.dialog())
        .expect("the agent sees it");
    assert_eq!(asked.kind, DialogKind::FileChooser);
    assert_eq!(asked.message, "The page asks for files (image/*).");
    let read = view.read_with(cx, |view, _| view.chooser_asked());
    assert_eq!(read.map(|asked| asked.mode), Some(FileMode::OpenMultiple));
    let read = read.expect("asked");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.ready_for_agents(cx), Ok(()), "it holds nothing up");
    });

    let files = vec!["/work/app/a.png".to_owned(), "/work/app/b.png".to_owned()];
    view.update(cx, |view, cx| {
        view.agent_chooses(&agent(), read, files.clone(), cx)
    })
    .expect("answered");
    assert_eq!(answers(&host, cx), [(7, Answer::Files { paths: files })]);
    assert_eq!(view.read_with(cx, |view, _| view.dialog()), None);
    let again = view.update(cx, |view, cx| view.agent_chooses(&agent(), read, vec![], cx));
    assert_eq!(again, Err(Unanswered::NothingAsked));
}

#[gpui_kit::test]
fn an_agents_chooser_is_cancelled_when_it_says_so_lets_go_or_the_person_takes_the_page(
    cx: &mut TestAppContext,
) {
    let (host, view, pane) = page("chooser-cancelled", true, cx);
    asks_files(&host, pane, 1, FileMode::Open, cx);
    let accepted = view.update(cx, |view, cx| {
        view.agent_answers(&agent(), DialogAction::Accept, None, cx)
    });
    assert_eq!(
        accepted,
        Err(Unanswered::FilesAsked),
        "only files answer it"
    );
    view.update(cx, |view, cx| {
        view.agent_answers(&agent(), DialogAction::Dismiss, None, cx)
    })
    .expect("cancelled");
    assert_eq!(answers(&host, cx), [(1, Answer::Dismiss)]);

    asks_files(&host, pane, 2, FileMode::Open, cx);
    view.update(cx, |view, cx| view.agent_releases(&agent(), cx));
    assert_eq!(
        answers(&host, cx),
        [(2, Answer::Dismiss)],
        "let go with the page"
    );

    view.update(cx, |view, cx| view.agent_claims(agent(), false, None, cx))
        .expect("free");
    asks_files(&host, pane, 3, FileMode::Open, cx);
    view.update(cx, |view, cx| view.person_acts(Mode::Implicit, cx));
    assert_eq!(
        answers(&host, cx),
        [(3, Answer::Dismiss)],
        "the person took the page"
    );

    view.update(cx, |view, cx| {
        view.nobody_has_it(cx);
        view.agent_claims(agent(), false, None, cx)
    })
    .expect("given back");
    asks_files(&host, pane, 4, FileMode::Open, cx);
    assert!(view.read_with(cx, |view, _| view.dialog()).is_some());
    host.says(HostEvent::DialogClosed { pane, dialog: 4 }, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.dialog()),
        None,
        "the page withdrew it"
    );
}

#[gpui_kit::test]
fn the_persons_chooser_is_theirs(cx: &mut TestAppContext) {
    let (host, view, pane) = page("chooser-persons", false, cx);
    asks_files(&host, pane, 5, FileMode::Open, cx);
    assert_eq!(
        view.read_with(cx, |view, _| (view.dialog(), view.chooser_asked())),
        (None, None)
    );
    let guessed = super::Asked {
        id: 5,
        mode: FileMode::Open,
    };
    let refused = view.update(cx, |view, cx| {
        view.agent_chooses(&agent(), guessed, vec!["/x".into()], cx)
    });
    assert_eq!(
        refused,
        Err(Unanswered::NothingAsked),
        "the system's panel asks the person"
    );
    assert_eq!(answers(&host, cx), []);
}

#[gpui_kit::test]
fn files_go_only_to_the_chooser_the_agent_read(cx: &mut TestAppContext) {
    let (host, view, pane) = page("chooser-read", true, cx);
    asks_files(&host, pane, 7, FileMode::Open, cx);
    let read = view.read_with(cx, |view, _| view.chooser_asked()).expect("asked");
    assert_eq!(read.mode, FileMode::Open);
    host.says(HostEvent::DialogClosed { pane, dialog: 7 }, cx);
    asks_files(&host, pane, 8, FileMode::Save, cx);
    let given = view.update(cx, |view, cx| {
        view.agent_chooses(&agent(), read, vec!["/work/app/a.png".into()], cx)
    });
    assert_eq!(given, Err(Unanswered::AskedAnew), "the page asks anew, where to save");
    assert_eq!(answers(&host, cx), [], "a chooser asking where to save gets no path");
}

#[gpui_kit::test]
fn an_agents_chooser_is_gone_once_another_host_serves_the_page(cx: &mut TestAppContext) {
    let (host, view, pane) = page("chooser-restart", true, cx);
    asks_files(&host, pane, 3, FileMode::Open, cx);
    assert!(view.read_with(cx, |view, _| view.chooser_asked()).is_some());
    let again = FakeHost::serve("chooser-restart-again", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.chooser_asked()),
        None,
        "the host serving the page now never asked it"
    );
    assert_eq!(view.read_with(cx, |view, _| view.dialog()), None);
    let answered = answers(&again, cx);
    assert!(answered.is_empty(), "nothing answered to a question of the new host's: {answered:?}");
}
