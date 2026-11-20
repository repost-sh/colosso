//! How light a page is and what its canvas is painted ([`Tone`]), which the app's glass over
//! the page and what's behind it take: the renderer's watcher works it out in the page's main
//! frame as the page first shows and as something that could change it happens, and says it
//! only when it changed (`renderer`); the host tells the app ([`HostEvent::Tone`]).
use cef::{CefString, ImplListValue, ImplProcessMessage, ProcessMessage};
use koyo_browser_ipc::{HostEvent, PaneId, Tone};

pub(crate) mod renderer;

/// The watcher's process messages' name.
const TOLD: &str = "koyo-tone";

/// A process message from a page's renderer: `true` when it was the watcher's, whose tone the
/// app is told.
pub(crate) fn from_renderer(pane: PaneId, message: &ProcessMessage) -> bool {
    if CefString::from(&message.name()).to_string() != TOLD {
        return false;
    }
    let said = (message.argument_list())
        .map(|args| CefString::from(&args.string(0)).to_string())
        .unwrap_or_default();
    if let Some(tone) = parse(&said) {
        crate::host::send(&HostEvent::Tone { pane, tone });
    }
    true
}

/// What the watcher said: the lightness at each point of the grid, comma-separated, row by
/// row, then after a semicolon the canvas's red, green and blue, space-separated; none if it's
/// of another shape. A canvas of another shape is white.
fn parse(said: &str) -> Option<Tone> {
    let (grid, canvas) = said.split_once(';').unwrap_or((said, ""));
    let lightness: Vec<f32> = grid.split(',').filter_map(|cell| cell.parse().ok()).collect();
    let parts: Vec<u8> = canvas.split(' ').filter_map(|part| part.parse().ok()).collect();
    let canvas = <[u8; 3]>::try_from(parts).unwrap_or([255; 3]);
    (lightness.len() == Tone::ACROSS * Tone::DOWN).then_some(Tone { lightness, canvas })
}

#[cfg(test)]
mod tests;
