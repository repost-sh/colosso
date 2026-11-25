//! Looking at a page a pane draws, which no agent uses: it says its tone for the glass over it
//! (`tone.rs`), and zoomed it's laid out afresh.

use std::sync::mpsc::Receiver;

use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{AppRequest, HostEvent, Tone, ViewAction, Zoom};
use serde_json::json;

use crate::asking::{evaluate, kept, until};
use crate::inside::{open, open_at, wait};
use crate::window;

/// The address of a page of nothing but `colour`.
fn page(colour: &str) -> String {
    format!("data:text/html,<title>{colour}</title><body style='margin:0;background:{colour}'></body>")
}

/// Painted white while it was hidden, the page a pane draws says it's white once it's drawn, and
/// the black page gone to in it that it's black.
pub fn tone(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open_at(host, incoming, 1, &page("gray"), "gray")?;
    window::within(|| {
        kept(host, false)?;
        until(host, incoming, "document.visibilityState", &json!("hidden"))?;
        evaluate(host, incoming, "document.body.style.background = 'white'")?;
        window::show(1)?;
        says(incoming, "white", |light| light > 0.95)?;
        let black = AppRequest::Navigate { pane: 1, url: page("black") };
        host.send(&black).map_err(|error| format!("navigate: {error}"))?;
        says(incoming, "black", |light| light < 0.05)
    })
}

/// Waits for the page to say it's as light as `light` holds, all over.
fn says(incoming: &Receiver<Incoming>, what: &str, light: impl Fn(f32) -> bool) -> Result<(), String> {
    wait(incoming, &format!("the page to say it's {what}"), |event| match event {
        HostEvent::Tone { pane: 1, tone } => {
            let cells = &tone.lightness;
            (cells.len() == Tone::ACROSS * Tone::DOWN && cells.iter().all(|cell| light(*cell))).then_some(())
        }
        _ => None,
    })
}

/// Zoomed to twice its size, the page a pane draws has half the room, and all of it again back
/// at its own.
pub fn zoom(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    window::within(|| {
        window::show(1)?;
        kept(host, false)?;
        until(host, incoming, "document.visibilityState", &json!("visible"))?;
        let room = "[innerWidth, innerHeight]";
        until(host, incoming, room, &json!([400, 300]))?;
        for (percent, has) in [(200, json!([200, 150])), (100, json!([400, 300]))] {
            let action = ViewAction::Zoom { level: Zoom(percent).level() };
            host.send(&AppRequest::View { pane: 1, action }).map_err(|error| format!("zoom: {error}"))?;
            until(host, incoming, room, &has)?;
            eprintln!("at {percent}% the page has {has}");
        }
        Ok(())
    })
}
