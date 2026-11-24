//! A page that asks the person to confirm leaving it (`beforeunload`): Chrome asks only for a
//! page the person used, so each page here is given a gesture first.
use koyo_browser_ipc::{AppRequest, HostEvent};

use crate::agents;
use crate::support::{Session, close, launch, open};

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

/// A page the app closes goes, though it asks to confirm leaving: the app let it go, so
/// nobody is there to answer, and the host answers for it.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_the_app_closes_goes_though_it_asks_to_confirm_leaving() {
    let launch = launch("closes-asking");
    let mut session = Session::start(&launch);
    open(&mut session, ASKS, "asks");
    used(&mut session);
    (session.host.send(&AppRequest::Close { pane: 1 })).expect("close");
    session.wait("the page to close", |event| match event {
        HostEvent::Dialog { pane: 1, question, .. } => {
            panic!("the app was asked {question:?} for a page it closed")
        }
        HostEvent::Closed { pane: 1 } => true,
        _ => false,
    });
    close(session, &launch);
}
