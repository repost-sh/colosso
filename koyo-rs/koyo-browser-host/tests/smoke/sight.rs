//! A page out of sight is hidden to Chromium, which slows it down as it does a tab in the
//! background, never to fewer than a tick of its timers a second, and draws at once when it's
//! back.

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

    (session.host.send(&AppRequest::Sight { pane: 1, kept: false, seen: true })).expect("visible");
    becomes(&mut session, "hidden");
    let hidden = ticks(&mut session);
    eprintln!("ticks in two seconds: {shown} on screen, {hidden} out of sight");
    assert!(hidden <= 5, "{hidden} ticks in two seconds out of sight");

    let back = Instant::now();
    (session.host.send(&AppRequest::Sight { pane: 1, kept: true, seen: true })).expect("visible");
    let frame = "new Promise(done => requestAnimationFrame(() => done(document.visibilityState)))";
    let drawn = evaluate(&mut session, frame);
    eprintln!("the first frame back came {:?} after it was asked for", back.elapsed());
    assert_eq!(drawn, "visible", "the first frame back is of a visible page");
    let shown = ticks(&mut session);
    assert!(shown > 100, "{shown} ticks in two seconds back on screen");
    close(session, &launch);
}

/// A page whose timer of a second keeps when each tick came, and that never paints anything
/// contentful: Chromium takes it to be loading still, which is what its grace before it
/// throttles a hidden page's timers to once a minute is set by (`grace_period_seconds`). A
/// page that painted waits a fixed minute.
const EACH_SECOND: &str = "data:text/html,<title>each second</title>\
    <script>window.at=[];setInterval(()=>at.push(performance.now()),1000)</script>";

/// The longest the page's timer went without running in the last `seconds`, up to now: the
/// gaps between its ticks, and the time since the last.
const LONGEST: &str = "((seconds) => { const now = performance.now(); \
    const ticks = at.filter(t => t > now - seconds * 1000).concat([now]); \
    return Math.max(...ticks.slice(1).map((t, i) => t - ticks[i])) / 1000; })(20)";

/// Chromium's grace is five seconds here, which it takes only while its throttling is on:
/// past it, the hidden page's timer still ticks each second, where it waited for the minute.
#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_silent_page_out_of_sight_past_chromes_grace_still_runs_its_timer_each_second() {
    let launch = launch("wake-ups");
    let grace = "enable-features=IntensiveWakeUpThrottling:grace_period_seconds/5";
    let mut session = Session::start_with(&launch, &[("KOYO_NATIVE_SWITCHES", grace)]);
    open(&mut session, EACH_SECOND, "each second");
    (session.host.send(&AppRequest::Sight { pane: 1, kept: false, seen: true })).expect("visible");
    becomes(&mut session, "hidden");
    session.pump(Instant::now() + Duration::from_secs(30), |_| false);
    let longest = evaluate(&mut session, LONGEST).as_f64().expect("seconds");
    eprintln!("the longest the timer of a second waited, 10 to 30 s out of sight: {longest:.2} s");
    assert!(longest < 2.5, "the timer of a second waited {longest:.2} s out of sight");
    close(session, &launch);
}
