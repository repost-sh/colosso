//! A page that asks the person to confirm leaving it (`beforeunload`): Chrome asks only for a
//! page the person used, so each page here is given a gesture first.
use std::time::{Duration, Instant};

use koyo_browser_ipc::client::Launch;
use koyo_browser_ipc::{Answer, AppRequest, Dialog, HostEvent};

use crate::agents;
use crate::support::{Session, close, evaluate, launch, open};

/// A page that asks to confirm leaving, as `music.youtube.com` does while it plays.
const ASKS: &str = "data:text/html,<title>asks</title><script>\
    addEventListener('beforeunload', e => { e.preventDefault(); e.returnValue = ''; })</script>";

/// Gives pane 1's page a gesture of the person's, without which it may not ask.
fn used(session: &mut Session) {
    does(session, "1");
}

/// Has pane 1's page run `script` as of a gesture of the person's.
fn does(session: &mut Session, script: &str) {
    let script = serde_json::json!({ "expression": script, "userGesture": true });
    agents::cdp(session, 800, "Runtime.evaluate", script);
}

/// A session whose pane 1 is open at [`ASKS`], its page used.
fn asking(launch: &Launch) -> Session {
    let mut session = Session::start(launch);
    open(&mut session, ASKS, "asks");
    used(&mut session);
    session
}

/// Closes pane 1 as the app does, and waits for its page to go, nobody asked anything.
fn goes_unasked(session: &mut Session) {
    session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    session.wait("the page to close", |event| match event {
        HostEvent::Dialog { pane: 1, question, .. } => panic!("the app was asked {question:?}"),
        HostEvent::LeaveAnswered { pane: 1, .. } => panic!("nobody was asked"),
        HostEvent::TabClosed { pane: 1 } => panic!("the page the app closed closed itself"),
        HostEvent::Closed { pane: 1 } => true,
        _ => false,
    });
}

/// A page the app closes goes, though it asks to confirm leaving: nobody is there to answer.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_app_closes_goes_though_it_asks_to_confirm_leaving() {
    let launch = launch("closes-asking");
    let mut session = asking(&launch);
    goes_unasked(&mut session);
    close(session, &launch);
}

/// Closes pane 1 as the person does, and waits for its page to ask them to confirm leaving:
/// the question's number.
fn leave_asking(session: &mut Session) -> u64 {
    (session.host.send(&AppRequest::Leave { pane: 1 })).expect("leave");
    question(session, true)
}

/// Waits for pane 1's page to ask the person to confirm leaving it, as they close it when
/// `close` and for a link otherwise: the question's number.
fn question(session: &mut Session, close: bool) -> u64 {
    let mut asked = None;
    session.wait("the page to ask to confirm leaving", |event| match event {
        HostEvent::Dialog { pane: 1, dialog, question: Dialog::BeforeUnload { reload: false, closing }, .. }
            if *closing == close =>
        {
            asked = Some(*dialog);
            true
        }
        HostEvent::Closed { pane: 1 } => panic!("the page closed without asking the person"),
        HostEvent::LeaveAnswered { pane: 1, .. } => panic!("nobody has answered"),
        _ => false,
    });
    asked.expect("a question")
}

/// Answers pane 1's question `dialog` with `answer`, as the person does.
fn answer(session: &mut Session, dialog: u64, answer: Answer) {
    (session.host.send(&AppRequest::Answer { pane: 1, dialog, answer })).expect("answer");
}

/// Waits for pane 1's page to say the person may leave it.
fn may_leave(session: &mut Session) {
    session.wait("the page to say it may be left", |event| match event {
        HostEvent::Closed { pane: 1 } => panic!("the page closed"),
        HostEvent::LeaveAnswered { pane: 1, may: true } => true,
        _ => false,
    });
}

/// A page that asks to confirm leaving asks the person, and may be left once they leave: it's
/// open still, for the app to close, and asks again when it isn't closed after all.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_person_leaves_asks_them_and_stays_open_for_the_app_to_close() {
    let launch = launch("leaves-asking");
    let mut session = asking(&launch);
    for _ in 0..2 {
        let asked = leave_asking(&mut session);
        answer(&mut session, asked, Answer::Accept);
        may_leave(&mut session);
        assert_eq!(evaluate(&mut session, "document.title"), "asks", "the page is still there");
    }
    goes_unasked(&mut session);
    close(session, &launch);
}

/// A page that asks the person is not left by asking it again, however long its question is up,
/// nor does it ask twice.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_asking_the_person_asked_again_waits_for_their_answer() {
    let launch = launch("asked-again");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    for _ in 0..2 {
        session.host.send(&AppRequest::Leave { pane: 1 }).expect("leave");
    }
    let quiet = Instant::now() + Duration::from_secs(1);
    session.pump(quiet, |event| match event {
        HostEvent::LeaveAnswered { .. } | HostEvent::Closed { .. } | HostEvent::Dialog { .. } => {
            panic!("it waits for the answer, and said {event:?}")
        }
        _ => false,
    });
    answer(&mut session, asked, Answer::Accept);
    may_leave(&mut session);
    goes_unasked(&mut session);
    close(session, &launch);
}

/// A page the person stays on says so, and the app can close it still.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_person_stays_on_stays_open_until_closed() {
    let launch = launch("stays");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    answer(&mut session, asked, Answer::Dismiss);
    session.wait("the person to stay", |event| matches!(event, HostEvent::LeaveAnswered { pane: 1, may: false }));
    assert_eq!(evaluate(&mut session, "document.title"), "asks", "the page is still there");
    goes_unasked(&mut session);
    close(session, &launch);
}

/// A page that doesn't ask to confirm leaving may be left at once, and is open still.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_that_does_not_ask_may_be_left_at_once() {
    let launch = launch("leaves");
    let mut session = Session::start(&launch);
    open(&mut session, "data:text/html,<title>quiet</title>", "quiet");
    used(&mut session);
    session.host.send(&AppRequest::Leave { pane: 1 }).expect("leave");
    may_leave(&mut session);
    assert_eq!(evaluate(&mut session, "document.title"), "quiet", "the page is still there");
    goes_unasked(&mut session);
    close(session, &launch);
}

/// A page still asking the person goes once the app closes it, their question with it.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_asking_the_person_goes_once_the_app_lets_it_go() {
    let launch = launch("let-go-asking");
    let mut session = asking(&launch);
    let asked = leave_asking(&mut session);
    session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    let mut withdrawn = false;
    session.wait("the page to close", |event| {
        withdrawn |= matches!(event, HostEvent::DialogClosed { pane: 1, dialog } if *dialog == asked);
        assert!(!matches!(event, HostEvent::LeaveAnswered { .. }), "nobody said so");
        matches!(event, HostEvent::Closed { pane: 1 })
    });
    assert!(withdrawn, "the person's question goes with the page");
    close(session, &launch);
}

/// A page whose unload handler runs as the app closes it is never asked whether it may be left.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_app_closes_is_not_asked_whether_it_may_be_left() {
    let launch = launch("closes-unasked");
    let mut session = asking(&launch);
    let told = "addEventListener('beforeunload', () => console.info('asked')); 1";
    evaluate(&mut session, told);
    agents::cdp(&mut session, 801, "Runtime.enable", serde_json::json!({}));
    session.host.send(&AppRequest::Close { pane: 1 }).expect("close");
    session.wait("the page to close", |event| match event {
        HostEvent::CdpEvent { pane: 1, method, .. } if method == "Runtime.consoleAPICalled" => {
            panic!("the page was asked")
        }
        HostEvent::Closed { pane: 1 } => true,
        _ => false,
    });
    close(session, &launch);
}

/// A page that asks the person for a link they followed isn't asked whether it may be left, which
/// the host says, and is once they've answered.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_asking_for_a_link_is_not_left_by_closing_it() {
    let launch = launch("asks-for-a-link");
    let mut session = asking(&launch);
    does(&mut session, "location.href = 'about:blank'; 1");
    let asked = question(&mut session, false);
    session.host.send(&AppRequest::Leave { pane: 1 }).expect("leave");
    session.wait("the page to stay", |event| matches!(event, HostEvent::LeaveAnswered { pane: 1, may: false }));
    answer(&mut session, asked, Answer::Dismiss);
    let again = leave_asking(&mut session);
    answer(&mut session, again, Answer::Accept);
    may_leave(&mut session);
    goes_unasked(&mut session);
    close(session, &launch);
}
