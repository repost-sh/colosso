//! A page out of sight is hidden to Chromium, which slows it down as it does a tab in the
//! background, and draws at once when it's back.

use koyo_browser_ipc::AppRequest;
use std::time::{Duration, Instant};

use crate::support::{Session, close, evaluate, launch, open};

/// A page counting the ticks of a timer every 10 ms.
const TICKING: &str = "data:text/html,<title>ticking</title><body>\
    <script>window.ticks=0;setInterval(()=>window.ticks++,10)</script>";

/// How many ticks the page's timer makes in two seconds, timed by the test, not by the page,
/// whose timers are what's measured.
fn ticks(session: &mut Session) -> u64 {
    let count = |session: &mut Session| evaluate(session, "window.ticks").as_u64().expect("a count");
    let from = count(session);
    session.pump(Instant::now() + Duration::from_secs(2), |_| false);
    count(session) - from
}

/// Waits for the page's `visibilityState` to be `state`, asked every 50 ms of the test's: a
/// page just hidden sometimes hadn't run a timer of 50 ms 30 s on, so the page's own timers
/// don't time this.
fn becomes(session: &mut Session, state: &'static str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while evaluate(session, "document.visibilityState") != state {
        assert!(Instant::now() < deadline, "timed out waiting for the page to be {state}");
        session.pump(Instant::now() + Duration::from_millis(50), |_| false);
    }
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_out_of_sight_runs_its_timers_slowly_and_draws_at_once_when_back() {
    let launch = launch("sight");
    let mut session = Session::start(&launch);
    open(&mut session, TICKING, "ticking");
    let shown = ticks(&mut session);
    assert!(shown > 100, "{shown} ticks in two seconds on screen");

    (session.host.send(&AppRequest::Visible { pane: 1, visible: false })).expect("visible");
    becomes(&mut session, "hidden");
    let hidden = ticks(&mut session);
    eprintln!("ticks in two seconds: {shown} on screen, {hidden} out of sight");
    assert!(hidden <= 5, "{hidden} ticks in two seconds out of sight");

    let back = Instant::now();
    (session.host.send(&AppRequest::Visible { pane: 1, visible: true })).expect("visible");
    let frame = "new Promise(done => requestAnimationFrame(() => done(document.visibilityState)))";
    let drawn = evaluate(&mut session, frame);
    eprintln!("the first frame back came {:?} after it was asked for", back.elapsed());
    assert_eq!(drawn, "visible", "the first frame back is of a visible page");
    let shown = ticks(&mut session);
    assert!(shown > 100, "{shown} ticks in two seconds back on screen");
    close(session, &launch);
}
