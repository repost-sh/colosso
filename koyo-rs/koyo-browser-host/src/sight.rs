//! What the log says of a page being seen and heard (target `koyo::sight`), one line for each
//! change and none in between: the engine told the page is hidden or shown, and the page
//! starting or stopping playing sound. Chromium spares a page that plays from what it does to
//! a hidden one, so the two together tell why a hidden player stopped. A line names the pane
//! and its address's host, never the rest of the address.
//!
//! CEF tells a Chrome-style client nothing of a page's audio, so the page says it: the
//! renderer's watcher hears its media elements play and stop (`renderer`).
use cef::{CefString, Frame, ImplFrame, ImplListValue, ImplProcessMessage, ProcessMessage};
use koyo_browser_ipc::PaneId;

pub(crate) mod renderer;

/// The log's target.
const TARGET: &str = "koyo::sight";

/// The watcher's process messages' name.
const HEARD: &str = "koyo-heard";

/// What the watcher says as the page starts playing; anything else it says is the event it
/// stopped at.
const PLAYS: &str = "plays";

/// The engine was told `pane`'s page is shown, or hidden.
pub(crate) fn shown(pane: PaneId, shown: bool) {
    let site = site(crate::host::address_of(pane).as_deref());
    let now = if shown { "shown" } else { "hidden" };
    log::info!(target: TARGET, "pane {pane} ({site}) {now} to the engine");
}

/// A process message from a page's renderer: `true` when it was the watcher's, which the log
/// is told.
pub(crate) fn from_renderer(pane: PaneId, frame: &Frame, message: &ProcessMessage) -> bool {
    if CefString::from(&message.name()).to_string() != HEARD {
        return false;
    }
    let said = (message.argument_list())
        .map(|args| CefString::from(&args.string(0)).to_string())
        .unwrap_or_default();
    let site = site(Some(&CefString::from(&frame.url()).to_string()));
    if said == PLAYS {
        log::info!(target: TARGET, "pane {pane} ({site}) plays sound");
    } else {
        log::info!(target: TARGET, "pane {pane} ({site}) stopped playing: {said}");
    }
    true
}

/// The host of `address`, else its scheme, else `-`.
fn site(address: Option<&str>) -> String {
    let Some(url) = address.and_then(|address| url::Url::parse(address).ok()) else {
        return "-".into();
    };
    url.host_str().map_or_else(|| format!("{}:", url.scheme()), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::site;

    #[test]
    fn a_line_names_the_host_and_never_the_path_or_query() {
        let music = Some("https://music.youtube.com/watch?v=abc&list=xyz#t=10");
        assert_eq!(site(music), "music.youtube.com");
        assert_eq!(site(Some("data:text/html,<title>secret</title>")), "data:");
        assert_eq!(site(Some("not an address")), "-");
        assert_eq!(site(None), "-");
    }
}
