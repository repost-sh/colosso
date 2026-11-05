//! A page that asks where the computer is, and is allowed to know: the position, or why there
//! is none, reaches it.
//!
//! Chromium reads the position only once macOS lets the host know it, and macOS asks the
//! person about that once: the first run of an engine macOS hasn't asked about waits for
//! the person to answer macOS's prompt.

use koyo_browser_ipc::{Answer, AppRequest, Dialog, HostEvent, Permission};
use std::time::Duration;

use crate::support::{Session, close, evaluate, launch, open, serve, until};

/// A page that keeps what its request for a position came to.
const ASKS: &str = "<!doctype html><title>where</title><script>\
    var located = null;\
    function ask() {\
      navigator.geolocation.getCurrentPosition(\
        p => { located = { latitude: p.coords.latitude, longitude: p.coords.longitude }; },\
        e => { located = { code: e.code, message: e.message }; });\
    }</script>";

/// How long the person has to answer macOS, the first time.
const ANSWER: Duration = Duration::from_secs(180);

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_allowed_to_know_where_it_is_hears_back() {
    let launch = launch("location");
    let mut session = Session::start(&launch);
    let page = serve(ASKS);
    open(&mut session, &page, "where");
    // Asked once the evaluation has answered, so the prompt comes after.
    evaluate(&mut session, "setTimeout(ask, 200), true");
    let mut asked = None;
    session.wait("the page to ask for its position", |event, _| {
        if let HostEvent::Dialog {
            pane: 1,
            dialog,
            question: Dialog::Permission { permissions },
            ..
        } = event
            && permissions.contains(&Permission::Location)
        {
            asked = Some(*dialog);
            return true;
        }
        false
    });
    session
        .host
        .send(&AppRequest::Answer {
            pane: 1,
            dialog: asked.expect("asked"),
            answer: Answer::Accept,
        })
        .expect("answer");
    let located = heard(&mut session);
    assert!(
        located["latitude"].is_number(),
        "the page was refused its position: {located}"
    );

    // Allowed already: the page asks again without a prompt, and hears back.
    let again = format!("{page}?again");
    session
        .host
        .send(&AppRequest::Navigate {
            pane: 1,
            url: again.clone(),
        })
        .expect("navigate");
    session.wait("the page to load again", |event, _| {
        matches!(event, HostEvent::Page { pane: 1, state } if state.url == again && !state.loading)
    });
    evaluate(&mut session, "ask(), true");
    let located = heard(&mut session);
    assert!(
        located["latitude"].is_number(),
        "the page was refused its position the second time: {located}"
    );
    close(session, &launch);
}

/// What the page's request for a position came to, once it hears back.
fn heard(session: &mut Session) -> serde_json::Value {
    until(
        session,
        "JSON.stringify(located)",
        "the page to hear back about its position",
        ANSWER,
        |located| !located.is_null(),
    )
}
