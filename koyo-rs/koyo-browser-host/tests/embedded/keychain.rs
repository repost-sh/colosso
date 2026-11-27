//! The key the engine keeps its profiles' cookies and passwords under: on Koyo's own data
//! folder the one they were saved under, and on any other Chromium's protection of it
//! (`app/switches.rs`), as the browser process's command line on `chrome://version` says.

use std::sync::mpsc::Receiver;

use koyo_browser_ipc::client::{Connection, Incoming};

use crate::asking::evaluate;
use crate::inside::open_at;

/// The feature that keeps a profile outside Chrome's own data directory from the key it was
/// saved under.
const PROTECTION: &str = "KeychainKeyProviderUserDataDirProtection";

/// The features the browser process runs with off, as `chrome://version` shows its command
/// line: CEF's `--disable-features` comes first, and the app's, which Chromium takes, last.
fn features_off(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<Vec<String>, String> {
    open_at(host, incoming, 1, "chrome://version", "About Version")?;
    let page = evaluate(host, incoming, "document.body.innerText")?;
    let page = page.as_str().ok_or_else(|| format!("chrome://version read as {page}"))?;
    let off = (page.split_whitespace())
        .filter_map(|switch| switch.strip_prefix("--disable-features="))
        .next_back()
        .ok_or_else(|| format!("no --disable-features on chrome://version: {page}"))?;
    if !off.split(',').any(|feature| feature == "HeapProfilerReporting") {
        return Err(format!("the app's --disable-features isn't the last on chrome://version: {page}"));
    }
    Ok(off.split(',').map(str::to_owned).collect())
}

/// On Koyo's own data folder the protection is off: what the profiles saved stays readable.
pub fn own_folder(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    let off = features_off(host, incoming)?;
    if !off.iter().any(|feature| feature == PROTECTION) {
        return Err(format!("on Koyo's own data folder {PROTECTION} is on: off are {off:?}"));
    }
    Ok(())
}

/// On another data folder the protection stays on.
pub fn another_folder(host: &mut Connection, incoming: &Receiver<Incoming>) -> Result<(), String> {
    let off = features_off(host, incoming)?;
    if off.iter().any(|feature| feature == PROTECTION) {
        return Err(format!("on another data folder {PROTECTION} is off: off are {off:?}"));
    }
    Ok(())
}
