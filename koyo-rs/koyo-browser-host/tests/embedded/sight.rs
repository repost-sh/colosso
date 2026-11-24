//! A page's sight as its views leave the app's window: it turns hidden once, not visible again
//! on its way out. (A page shown in a window macOS has covered turns visible after Chromium's
//! own second, `docs/native-pages.md` 5; one shown in a window the person sees, at once.)

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use koyo_browser_host::{embedded, native};
use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{AppRequest, CdpCaller, HostEvent, PaneId};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::bubbles::made;
use crate::inside::{open, open_as, wait};
use crate::pump::idle;

/// Has the page keep each visibility it changes to.
const CHANGES: &str = "window.changes = []; document.addEventListener('visibilitychange', () => { \
    window.changes.push(document.visibilityState); }); document.visibilityState";

/// Longer than Chromium waits before it takes a page in a covered window to be covered.
const SETTLED: Duration = Duration::from_secs(2);

/// A page's place in the app's window.
const PLACE: NSSize = NSSize::new(400., 300.);

/// The app's window and the views its pages are placed in, by pane.
type Shown = (Retained<NSWindow>, Vec<(PaneId, Retained<NSView>)>);

thread_local! {
    /// The app's window while a test shows pages in it, on the main thread.
    static WINDOW: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// The page shown in the app's window, then left as Koyo leaves a tab: the next tab's page put
/// in its place over it, its pane let go while the host still has it on screen, then the page
/// put out of sight. It turns hidden, once.
pub fn left(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    open_as(host, incoming, 2)?;
    on_main(|main| shown_in_window(main, &[1]))?;
    let outcome = leave(host, incoming);
    on_main(let_go)?;
    outcome
}

fn leave(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    idle(Duration::from_secs(1));
    let state = evaluate(host, incoming, 1, CHANGES)?;
    if state != "visible" {
        return Err(format!("the page shown was {state}"));
    }
    on_main(over_it)?;
    idle(Duration::from_millis(300));
    on_main(|_| {
        WINDOW.with(|kept| {
            let kept = kept.borrow();
            let (_, places) = kept.as_ref().ok_or("no app's window")?;
            for (pane, place) in places.iter().filter(|(pane, _)| *pane == 1) {
                native::release(*pane, place);
            }
            Ok(())
        })
    })?;
    visible(host, false)?;
    idle(SETTLED);
    let changes = evaluate(host, incoming, 2, "window.changes")?;
    eprintln!("leaving the app's window, the page went {changes}");
    if changes != serde_json::json!(["hidden"]) {
        return Err(format!("leaving the app's window, the page went {changes}, not hidden once"));
    }
    Ok(())
}

/// Where the app's window of the test's is.
/// Shows `panes`' pages in an app's window of the test's, each in a view of its own, the
/// window off every screen, so nothing shows on the person's.
fn shown_in_window(main: MainThreadMarker, panes: &[PaneId]) -> Result<(), String> {
    let count = f64::from(u32::try_from(panes.len()).map_err(|error| error.to_string())?);
    let window = made(main, NSSize::new(PLACE.width, PLACE.height * count));
    let content = window.contentView().ok_or("the app's window has no view")?;
    let origin = window.frame().origin;
    let mut places = Vec::new();
    for (row, pane) in (0_u32..).zip(panes) {
        let frame = NSRect::new(NSPoint::new(0., PLACE.height * f64::from(row)), PLACE);
        let place = NSView::initWithFrame(main.alloc(), frame);
        content.addSubview(&place);
        let screen = NSRect::new(NSPoint::new(origin.x, origin.y + frame.origin.y), PLACE);
        native::place(*pane, &place, &content, screen);
        places.push((*pane, place));
    }
    window.orderBack(None);
    WINDOW.with(|kept| kept.replace(Some((window, places))));
    Ok(())
}

/// Puts pane 2's page in the app's window where pane 1's is, over it, as the tab switched to
/// is put where the one left was.
fn over_it(main: MainThreadMarker) -> Result<(), String> {
    WINDOW.with(|kept| {
        let mut kept = kept.borrow_mut();
        let (window, places) = kept.as_mut().ok_or("no app's window")?;
        let content = window.contentView().ok_or("the app's window has no view")?;
        let origin = window.frame().origin;
        let frame = NSRect::new(NSPoint::new(0., 0.), PLACE);
        let place = NSView::initWithFrame(main.alloc(), frame);
        content.addSubview(&place);
        let screen = NSRect::new(origin, PLACE);
        native::place(2, &place, &content, screen);
        places.push((2, place));
        Ok(())
    })
}

/// Lets go of the pages in the app's window, and takes the window down.
fn let_go(_: MainThreadMarker) -> Result<(), String> {
    let (window, places) = WINDOW.with(RefCell::take).ok_or("no app's window")?;
    for (pane, place) in &places {
        native::release(*pane, place);
    }
    window.orderOut(None);
    Ok(())
}

/// Runs `work` on the main thread, and waits for what it comes to.
fn on_main<T: Send + 'static>(
    work: impl FnOnce(MainThreadMarker) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (done, said) = mpsc::channel();
    embedded::on_main(move || {
        let outcome = MainThreadMarker::new().ok_or_else(|| "not on the main thread".to_owned()).and_then(work);
        let _ = done.send(outcome);
    });
    said.recv_timeout(Duration::from_secs(10))
        .map_err(|error| format!("the main thread didn't answer: {error}"))?
}

fn visible(host: &mut Connection, visible: bool) -> Result<(), String> {
    (host.send(&AppRequest::Sight { pane: 1, kept: visible, seen: true })).map_err(|error| format!("visible: {error}"))
}

/// What `expression` comes to in the page.
fn evaluate(
    host: &mut Connection,
    incoming: &Receiver<Incoming>,
    call: u64,
    expression: &str,
) -> Result<serde_json::Value, String> {
    let request = AppRequest::Cdp {
        pane: 1,
        call,
        caller: CdpCaller::Agent,
        session: None,
        method: "Runtime.evaluate".into(),
        params: serde_json::json!({ "expression": expression, "returnByValue": true }),
    };
    host.send(&request).map_err(|error| format!("evaluate: {error}"))?;
    let (result, error) = wait(incoming, expression, |event| match event {
        HostEvent::CdpReply { call: answered, result, error, .. } if *answered == call => {
            Some((result.clone(), error.clone()))
        }
        _ => None,
    })?;
    if let Some(error) = error {
        return Err(format!("{expression}: {error}"));
    }
    Ok(result.map(|result| result["result"]["value"].clone()).unwrap_or_default())
}
