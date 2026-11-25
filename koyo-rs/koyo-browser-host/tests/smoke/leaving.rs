//! A page that asks the person to confirm leaving it (`beforeunload`): Chrome asks only for a
//! page the person used, so each page here is given a gesture first.
use koyo_browser_ipc::client::Launch;
use koyo_browser_ipc::{Answer, AppRequest, Dialog, HostEvent};

use crate::agents;
use crate::support::{Session, close, evaluate, launch, open};

/// A page that asks to confirm leaving, as `music.youtube.com` does while it plays.
const ASKS: &str = "data:text/html,<title>asks</title><script>\
    addEventListener('beforeunload', e => { e.preventDefault(); e.returnValue = ''; })</script>";

/// Gives pane 1's page a gesture of the person's, without which it may not ask.
fn used(session: &mut Session) {
    agents::cdp(
        session,
        800,
        "Runtime.evaluate",
        serde_json::json!({"expression": "1", "userGesture": true}),
    );
}

/// A session whose pane 1 is open at [`ASKS`], its page used.
fn asking(launch: &Launch) -> Session {
    let mut session = Session::start(launch);
    open(&mut session, ASKS, "asks");
    used(&mut session);
    session
}

/// Closes pane 1 with `close`, and waits for its page to go, nobody asked anything.
fn goes_unasked(session: &mut Session, close: &AppRequest) {
    session.host.send(close).expect("close");
    session.wait("the page to close", |event| match event {
        HostEvent::Dialog { pane: 1, question, .. } => panic!("the app was asked {question:?}"),
        HostEvent::Closed { pane: 1 } => true,
        _ => false,
    });
}

/// A page the app closes goes, though it asks to confirm leaving: the app let it go, so
/// nobody is there to answer, and the host answers for it.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_app_closes_goes_though_it_asks_to_confirm_leaving() {
    let launch = launch("closes-asking");
    let mut session = asking(&launch);
    goes_unasked(&mut session, &AppRequest::Close { pane: 1 });
    close(session, &launch);
}

/// Closes pane 1 as the person does, and waits for its page to ask them to confirm leaving:
/// the question's number.
fn leave_asking(session: &mut Session) -> u64 {
    (session.host.send(&AppRequest::Leave { pane: 1 })).expect("leave");
    let mut asked = None;
    session.wait("the page to ask to confirm leaving", |event| match event {
        HostEvent::Dialog { pane: 1, dialog, question: Dialog::BeforeUnload { reload: false }, .. } => {
            asked = Some(*dialog);
            true
        }
        HostEvent::Closed { pane: 1 } => panic!("the page closed without asking the person"),
        _ => false,
    });
    asked.expect("a question")
}

/// Answers pane 1's question `dialog` with `answer`, as the person does.
fn answer(session: &mut Session, dialog: u64, answer: Answer) {
    (session.host.send(&AppRequest::Answer { pane: 1, dialog, answer })).expect("answer");
}

/// A page the person closes that asks to confirm leaving asks them, and goes once they leave.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_person_closes_asks_them_and_goes_once_they_leave() {
    let launch = launch("leaves-asking");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    answer(&mut session, asked, Answer::Accept);
    session.wait("the page to close", |event| matches!(event, HostEvent::Closed { pane: 1 }));
    close(session, &launch);
}

/// A page the person closes that asks to confirm leaving stays when they stay, and the app can
/// close it still.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_person_stays_on_stays_open_until_closed() {
    let launch = launch("stays");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    answer(&mut session, asked, Answer::Dismiss);
    assert_eq!(evaluate(&mut session, "document.title"), "asks", "the page is still there");
    goes_unasked(&mut session, &AppRequest::Close { pane: 1 });
    close(session, &launch);
}

/// A page the person closes that doesn't ask to confirm leaving goes at once.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_that_does_not_ask_goes_at_once_as_the_person_closes_it() {
    let launch = launch("leaves");
    let mut session = Session::start(&launch);
    open(&mut session, "data:text/html,<title>quiet</title>", "quiet");
    used(&mut session);
    goes_unasked(&mut session, &AppRequest::Leave { pane: 1 });
    close(session, &launch);
}

/// A page the person is closing, its question still up, goes once the app lets it go: nobody is
/// left to answer, and the host answers for it.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_asking_the_person_goes_once_the_app_lets_it_go() {
    let launch = launch("let-go-asking");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    (session.host.send(&AppRequest::Close { pane: 1 })).expect("close");
    let mut withdrawn = false;
    session.wait("the page to close", |event| {
        withdrawn |= matches!(event, HostEvent::DialogClosed { pane: 1, dialog } if *dialog == asked);
        matches!(event, HostEvent::Closed { pane: 1 })
    });
    assert!(withdrawn, "the person's question goes with the page");
    close(session, &launch);
}
