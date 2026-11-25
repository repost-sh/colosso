//! A download the app saved opens no window of Chromium's over the page: Chrome's own
//! downloads bubble, which pops up from its toolbar as a download ends, stays down, the app
//! showing its downloads itself (`docs/browser.md` 16.3).

use std::sync::mpsc::Receiver;
use std::time::Duration;

use koyo_browser_ipc::client::{Connection, Incoming};
use koyo_browser_ipc::{AppRequest, CdpCaller, DownloadAct, DownloadState, HostEvent};
use objc2_app_kit::NSApplication;

use crate::inside::{open, wait};
use crate::pump::idle;
use crate::window::on_main;

/// The page downloads a file of its own making.
const DOWNLOAD: &str = "(() => { const a = document.createElement('a'); \
    a.href = URL.createObjectURL(new Blob(['hi'])); a.download = 'hi.txt'; a.click(); return true; })()";

/// How long after the download ended a bubble would have opened by.
const SETTLE: Duration = Duration::from_secs(3);

/// The page downloads a file the app saves: no window opens as it ends.
pub fn no_bubble(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    open(host, incoming)?;
    let before = shown()?;
    let params = serde_json::json!({ "expression": DOWNLOAD });
    let click = AppRequest::Cdp { pane: 1, call: 1, caller: CdpCaller::Agent, session: None, method: "Runtime.evaluate".into(), params };
    host.send(&click).map_err(|error| format!("the click: {error}"))?;
    let download = wait(incoming, "the download to ask where it goes", |event| match event {
        HostEvent::DownloadAsked { pane: 1, download, .. } => Some(*download),
        _ => None,
    })?;
    let scratch = std::env::var_os(crate::INSIDE).ok_or("no scratch folder")?;
    let path = std::path::Path::new(&scratch).join("hi.txt").to_string_lossy().into_owned();
    let save = AppRequest::Download { download, act: DownloadAct::SaveTo { path } };
    host.send(&save).map_err(|error| format!("the answer: {error}"))?;
    wait(incoming, "the download to end", |event| match event {
        HostEvent::Download { download: d } if d.id == download && d.state != DownloadState::Going => {
            Some(())
        }
        _ => None,
    })?;
    idle(SETTLE);
    let opened: Vec<String> = shown()?.into_iter().filter(|window| !before.contains(window)).collect();
    if !opened.is_empty() {
        return Err(format!("windows opened as the download ended: {opened:?}"));
    }
    eprintln!("the download ended and no window opened over the page");
    Ok(())
}

/// The app's visible windows, each by its address, class and title.
fn shown() -> Result<Vec<String>, String> {
    on_main(|main| {
        let windows = NSApplication::sharedApplication(main).windows();
        let visible = windows.iter().filter(|window| window.isVisible());
        let named = visible.map(|window| format!("{:p} {} {:?}", &*window, window.class().name().to_string_lossy(), window.title().to_string()));
        Ok(named.collect())
    })
}
