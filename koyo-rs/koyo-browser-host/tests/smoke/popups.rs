//! Pages a page opens (`pane/popups.rs`): a link to a new tab and a sign-in's popup each get a
//! pane of their own and the page stays where it was; the popup keeps its opener, answers it
//! and closes itself, and can't move the window it's in; a ⌘- or a middle click opens behind
//! and a ⇧-click in front; a popup no gesture asked for is blocked; a link to a `data:` address
//! in a new tab, which Chrome blocks, leaves the tab saying the address it's at.

use koyo_browser_ipc::{HostEvent, PaneId};
use std::time::{Duration, Instant};

use crate::agents::cdp;
use crate::support::{Session, close, evaluate, launch, open, serve};

/// The page at `/` opens the others; `/popup` asks to be resized, answers its opener with how
/// wide it is and closes, and every other path is titled with itself.
const PAGE: &str = "<title>opener</title><body style='margin:0'>\
    <a id=plain href='/plain' style='display:block;width:200px;height:40px'>plain</a>\
    <a id=blank href='/blank' target=_blank>blank</a>\
    <a id=data href='data:text/html,blocked' target=_blank>data</a><script>\
    if (location.pathname === '/popup') {\
      resizeTo(160, 120); setTimeout(() => {\
        opener.postMessage(['signed in', innerWidth], '*'); setTimeout(() => close(), 100);\
      }, 300);\
    } else if (location.pathname !== '/') {\
      document.title = location.pathname;\
    } else {\
      window.heard = []; addEventListener('message', event => heard.push(event.data));\
      window.unasked = open('/unasked') === null;\
    }</script></body>";

/// What the host said of pages pane 1's page opened.
#[derive(Default)]
struct Opened {
    /// Each pane given to a page it opened, its address, and whether it's in front.
    tabs: Vec<(PaneId, String, bool)>,
    /// Each address it asked for in a tab beside, and whether in front.
    beside: Vec<(String, bool)>,
    /// The panes whose pages closed themselves, and those closed.
    closed_itself: Vec<PaneId>,
    closed: Vec<PaneId>,
    /// What the host said, shortened, for a test that times out to show.
    heard: Vec<String>,
}

impl Opened {
    fn note(&mut self, event: &HostEvent) {
        if !matches!(event, HostEvent::Cursor { .. } | HostEvent::Favicon { .. }) {
            self.heard.push(format!("{event:?}").chars().take(200).collect());
        }
        match event {
            HostEvent::TabOpened { pane, opener: 1, url, active } => {
                self.tabs.push((*pane, url.clone(), *active));
            }
            HostEvent::OpenUrl { pane: 1, url, front } => self.beside.push((url.clone(), *front)),
            HostEvent::TabClosed { pane } => self.closed_itself.push(*pane),
            HostEvent::Closed { pane } => self.closed.push(*pane),
            _ => {}
        }
    }

    fn tab(&self, path: &str) -> Option<(PaneId, bool)> {
        (self.tabs.iter()).find(|(_, url, _)| url.ends_with(path)).map(|(pane, _, active)| (*pane, *active))
    }
}

/// Runs `expression` in pane 1's page as if the person's gesture did, as a click is: its value.
/// What the host says meanwhile is noted.
fn with_gesture(session: &mut Session, opened: &mut Opened, call: u64, expression: &str) -> serde_json::Value {
    let params = serde_json::json!({
        "expression": expression,
        "returnByValue": true,
        "userGesture": true,
    });
    cdp(session, call, "Runtime.evaluate", params);
    let mut answer = None;
    watch(session, opened, expression, |event| match event {
        HostEvent::CdpReply { call: answered, result, error, .. } if *answered == call => {
            answer = Some((result.clone(), error.clone()));
            true
        }
        _ => false,
    });
    let (result, error) = answer.expect("answered");
    assert_eq!(error, None, "{expression}");
    result.expect("a result")["result"]["value"].clone()
}

/// Notes what the host says until `done` takes an event, for 30 s at most.
fn watch(session: &mut Session, opened: &mut Opened, what: &str, mut done: impl FnMut(&HostEvent) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let heard = session.pump(deadline, |event| {
        opened.note(event);
        done(event)
    });
    assert!(heard, "timed out waiting for {what}, having heard {:#?}", opened.heard);
}

/// Notes what the host says until `done` holds of what was noted, for 30 s at most.
fn until(session: &mut Session, opened: &mut Opened, what: &str, done: impl Fn(&Opened) -> bool) {
    if done(opened) {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let heard = session.pump(deadline, |event| {
        opened.note(event);
        done(opened)
    });
    assert!(heard, "timed out waiting for {what}, having heard {:#?}", opened.heard);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_page_opens_tabs_beside_it_and_a_popup_keeps_its_opener() {
    let launch = launch("popups");
    let mut session = Session::start(&launch);
    let address = serve(PAGE);
    open(&mut session, &address, "opener");
    assert_eq!(evaluate(&mut session, "window.unasked"), true, "a popup with no gesture is blocked");

    let mut opened = Opened::default();
    with_gesture(&mut session, &mut opened, 101, "document.getElementById('blank').click()");
    until(&mut session, &mut opened, "the link's tab", |opened| opened.tab("/blank").is_some());
    let (blank, front) = opened.tab("/blank").expect("the link's tab");
    assert!(front, "a link to a new tab opens in front");
    session.wait("the link's page", |event| {
        matches!(event, HostEvent::Page { pane, state } if *pane == blank && state.title == "/blank")
    });

    let plain = format!("{address}plain");
    let frames = "new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))";
    assert_eq!(evaluate(&mut session, frames), true, "the page draws, and takes input from then on");
    for (at, (button, modifiers, front)) in [("left", 4, false), ("middle", 0, false), ("left", 8, true)].into_iter().enumerate() {
        for (kind, call) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 2)] {
            let press = serde_json::json!({
                "type": kind, "x": 20, "y": 10, "button": button, "clickCount": 1, "modifiers": modifiers,
            });
            cdp(&mut session, 110 + 3 * at as u64 + call, "Input.dispatchMouseEvent", press);
        }
        let what = format!("the link {button}-clicked with modifiers {modifiers}");
        until(&mut session, &mut opened, &what, |opened| opened.beside.len() > at);
        assert_eq!(opened.beside[at], (plain.clone(), front), "{button} with modifiers {modifiers}");
    }

    let popup = "window.open('/popup', 'signin', 'width=400,height=500') !== null";
    let handed = with_gesture(&mut session, &mut opened, 104, popup);
    assert_eq!(handed, true, "window.open hands back the popup");
    until(&mut session, &mut opened, "the popup to close itself", |opened| {
        opened.tab("/popup").is_some_and(|(pane, _)| opened.closed.contains(&pane))
    });
    let (popup, front) = opened.tab("/popup").expect("the popup's tab");
    assert!(front, "a popup opens in front");
    assert!(opened.closed_itself.contains(&popup), "the popup's tab goes with it");
    let heard = evaluate(&mut session, "heard");
    assert_eq!(heard, serde_json::json!([["signed in", 400]]), "it answers its opener, its holder unmoved");

    assert_eq!(evaluate(&mut session, "location.pathname"), "/", "the page stayed where it was");
    let everywhere = (opened.tabs.iter().map(|(_, url, _)| url)).chain(opened.beside.iter().map(|(url, _)| url));
    assert!(everywhere.clone().all(|url| !url.ends_with("/unasked")), "{:?}", everywhere.collect::<Vec<_>>());
    close(session, &launch);
}

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn a_new_tab_whose_data_address_chrome_blocks_says_the_address_it_is_at() {
    let launch = launch("popups-data");
    let mut session = Session::start(&launch);
    let address = serve(PAGE);
    open(&mut session, &address, "opener");
    let mut opened = Opened::default();
    with_gesture(&mut session, &mut opened, 101, "document.getElementById('data').click()");
    until(&mut session, &mut opened, "the link's tab", |opened| opened.tab("blocked").is_some());
    let (tab, _) = opened.tab("blocked").expect("the link's tab");
    let mut last = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    let settled = session.pump(deadline, |event| {
        opened.note(event);
        match event {
            HostEvent::Page { pane, state } if *pane == tab => {
                last = Some((state.url.clone(), state.loading));
                !state.loading && state.url == "about:blank"
            }
            _ => false,
        }
    });
    assert!(settled, "the tab says where its page is, about:blank, as Chrome does: it said {last:?}");
    close(session, &launch);
}
