//! A page's sight as its views come to the app's window and leave it: hidden once as it leaves,
//! not visible again on its way out; visible as a pane draws it, for the first time or again;
//! and, out of the window, on screen for the engine while an agent uses it. (A page shown in a
//! window macOS has covered turns visible after Chromium's own second, `docs/native-pages.md` 5;
//! one shown in a window the person sees, at once.)

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use koyo_browser_ipc::client::{Connection, Incoming};
use serde_json::{Value, json};

use crate::asking::{cdp, evaluate, kept, until};
use crate::inside::{open, open_as, open_at};
use crate::pump::idle;
use crate::window;

/// Has the page keep each visibility it changes to.
const CHANGES: &str = "window.changes = []; document.addEventListener('visibilitychange', () => { \
    window.changes.push(document.visibilityState); });";

/// What the page is now.
const STATE: &str = "document.visibilityState";

/// Longer than Chromium waits before it takes a page in a covered window to be covered.
const SETTLED: Duration = Duration::from_secs(2);

/// A page to drive: a button in its top left counting presses and clicks.
const DRIVEN: &str = "data:text/html,<title>driven</title><body style='margin:0'>\
    <button style='position:fixed;left:0;top:0;width:200px;height:100px' \
    onmousedown='window.downs=(window.downs||0)+1' \
    onclick='window.clicks=(window.clicks||0)+1'>go</button>";

/// The page shown in the app's window, then left as Koyo leaves a tab: the next tab's page put
/// in its place over it, its pane let go while the host still has it on screen, then the page
/// told hidden. Put back, it's visible again and draws, no agent using it. It turns hidden once,
/// and visible once.
pub fn left(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    open_as(host, incoming, 2)?;
    window::within(|| {
        window::show(1)?;
        until(host, incoming, STATE, &json!("visible"))?;
        evaluate(host, incoming, CHANGES)?;
        window::show(2)?;
        idle(Duration::from_millis(300));
        window::leave(1)?;
        kept(host, false)?;
        idle(SETTLED);
        is(&evaluate(host, incoming, "window.changes")?, &json!(["hidden"]), "a page left went")?;
        window::show(1)?;
        until(host, incoming, "window.changes", &json!(["hidden", "visible"]))?;
        draws(host, incoming)?;
        idle(SETTLED);
        is(&evaluate(host, incoming, "window.changes")?, &json!(["hidden", "visible"]), "a page put back went")
    })
}

/// A page no pane has drawn that no agent uses any longer is hidden, and the first time a pane
/// draws it it's visible and draws.
pub fn first_shown(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    window::within(|| {
        kept(host, false)?;
        until(host, incoming, STATE, &json!("hidden"))?;
        window::show(1)?;
        until(host, incoming, STATE, &json!("visible"))?;
        draws(host, incoming)
    })
}

/// A page whose tab was left, and is hidden, is on screen for the engine as an agent uses it: a
/// screenshot answers, its frames run and a click lands at once. Left be, it's hidden again.
pub fn driven(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open_at(host, incoming, 1, DRIVEN, "driven")?;
    window::within(|| {
        window::show(1)?;
        kept(host, false)?;
        until(host, incoming, STATE, &json!("visible"))?;
        window::leave(1)?;
        until(host, incoming, STATE, &json!("hidden"))?;
        kept(host, true)?;
        let shot = cdp(host, incoming, "Page.captureScreenshot", json!({ "format": "png" }))?;
        if shot["data"].as_str().is_none_or(str::is_empty) {
            return Err(format!("a screenshot answered {shot}"));
        }
        is(&evaluate(host, incoming, STATE)?, &json!("visible"), "the page an agent uses is")?;
        draws(host, incoming)?;
        let pressed = Instant::now();
        for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
            let event = json!({ "type": kind, "x": 100, "y": 50, "button": "left", "clickCount": 1 });
            cdp(host, incoming, "Input.dispatchMouseEvent", event)?;
        }
        let took = pressed.elapsed();
        let heard = evaluate(host, incoming, "[window.downs, window.clicks]")?;
        eprintln!("a click took {took:?}, heard as {heard}");
        if heard != json!([1, 1]) || took > Duration::from_secs(1) {
            return Err(format!("a click took {took:?}, heard as {heard}"));
        }
        kept(host, false)?;
        until(host, incoming, STATE, &json!("hidden"))
    })
}

/// The page draws two animation frames, as one on screen does; hidden, it draws none.
fn draws(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    let two = "new Promise(done => requestAnimationFrame(() => requestAnimationFrame(() => done(true))))";
    evaluate(host, incoming, two).map(|_| ()).map_err(|error| format!("the page drew no frames: {error}"))
}

fn is(now: &Value, wanted: &Value, what: &str) -> Result<(), String> {
    if now == wanted { Ok(()) } else { Err(format!("{what} {now}, not {wanted}")) }
}
