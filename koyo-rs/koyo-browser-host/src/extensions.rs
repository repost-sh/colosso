//! Browser extensions (`docs/extensions.md`). Chromium runs them; the host is how the app
//! learns which a profile has and what each set its toolbar action to, and how a press on an
//! icon of the app's becomes Chrome's own action: its popup, or `action.onClicked`.
//!
//! It all goes over the `DevTools` protocol at the browser's level (`pipe.rs`), in the
//! `Extensions` domain, most of it Koyo's own additions to that domain
//! (`script/cef-patches/koyo-extensions-protocol.patch`): pages are named there by their CEF
//! browser's identifier, profiles by their folder's name. Only where pages are native
//! (`docs/native-pages.md`): an off-screen page is no tab to an extension, and has no toolbar
//! for a popup to hang from.
//!
//! Everything but the pipe's reader runs on CEF's UI thread.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use koyo_browser_ipc::{
    Extension, ExtensionAction, ExtensionChange, ExtensionSource, HostEvent, PaneId,
};
use serde_json::{Value, json};

use crate::host;

mod pipe;
mod popup;

/// What the host asked Chromium and waits to hear back about.
enum Asked {
    /// The extensions of every profile.
    Installed,
    /// Something whose answer says only whether it went: named for the log.
    Done(&'static str),
}

#[derive(Default)]
struct State {
    /// The id of the host's next message.
    next: i64,
    /// What was asked, by message id, until answered.
    asked: HashMap<i64, Asked>,
    /// The page told to Chromium as the one the person is on.
    active: Option<PaneId>,
    /// Whether a list was asked for and hasn't come: a change meanwhile asks once more.
    listing: bool,
    /// Whether the list changed while one was on its way.
    stale: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Makes the pipe as Chromium's command line is put together, and answers the switch value
/// that names its ends (`--remote-debugging-io-pipes`); none where the engine is CEF's own,
/// or the pipe couldn't be made, and then there are no extension controls.
pub(crate) fn pipe_switch() -> Option<String> {
    if !patched("devtools-pipe-descriptors") {
        log::info!("extensions: the engine is CEF's own, without Koyo's patches; their icons and popups are off");
        return None;
    }
    let switch = pipe::open(host::protocol_heard);
    if switch.is_none() {
        log::warn!("extensions: no DevTools pipe to the engine; their icons and popups are off");
    }
    switch
}

/// Whether the engine's framework was built with Koyo's patch `patch`
/// (`script/cef-patches/`): `script/cef` lists the patches it applied in the framework's
/// `Resources/koyo-patches.txt`, which goes wherever the framework does.
fn patched(patch: &str) -> bool {
    crate::own_bundle()
        .and_then(|engine| {
            let framework = crate::embedded::framework(std::path::Path::new(&engine));
            std::fs::read_to_string(framework.join("Resources/koyo-patches.txt")).ok()
        })
        .is_some_and(|list| list.lines().any(|line| line.trim() == patch))
}

/// Whether extensions have their icons and popups: the host talks to the engine about them.
pub(crate) fn on() -> bool {
    pipe::is_open()
}

/// The engine is up: it's asked to say what changes, and what's installed.
pub(crate) fn started() {
    if !pipe::is_open() {
        return;
    }
    ask("Extensions.koyoSetUpdatesEnabled", &json!({ "enabled": true }), Asked::Done("updates"));
    list();
}

/// The engine is about to stop: the pipe hangs up, which Chromium waits for.
pub(crate) fn stopping() {
    pipe::close();
    popup::forget();
}

/// Sends `method` with `params`, and keeps what its answer is for.
fn ask(method: &str, params: &Value, asked: Asked) {
    let id = STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.next += 1;
        let id = state.next;
        state.asked.insert(id, asked);
        id
    });
    let message = json!({ "id": id, "method": method, "params": params });
    if !pipe::send(&message.to_string()) {
        STATE.with(|state| state.borrow_mut().asked.remove(&id));
    }
}

/// A pane was opened in a profile for the first time since the host started: its extensions
/// are told to the app. Chromium may have had the profile open already, the one used last,
/// and says nothing new of it then.
pub(crate) fn profile_opened() {
    if !pipe::is_open() {
        return;
    }
    // What its extensions set for every page was said when the engine started, of a
    // profile the host couldn't name yet: turned off and on, the engine says it all again.
    for enabled in [false, true] {
        ask("Extensions.koyoSetUpdatesEnabled", &json!({ "enabled": enabled }), Asked::Done("updates"));
    }
    list();
}

/// Asks for every profile's extensions; one answer at a time.
fn list() {
    let already = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.listing {
            state.stale = true;
        }
        std::mem::replace(&mut state.listing, true)
    });
    if !already {
        ask("Extensions.koyoGetInstalled", &json!({}), Asked::Installed);
    }
}

/// A message Chromium wrote on the pipe: an answer to something asked, or an event.
pub(crate) fn heard(text: &str) {
    let Ok(message) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(id) = message.get("id").and_then(Value::as_i64) {
        let Some(asked) = STATE.with(|state| state.borrow_mut().asked.remove(&id)) else {
            return;
        };
        answered(&asked, &message);
        return;
    }
    let params = message.get("params").unwrap_or(&Value::Null);
    match message.get("method").and_then(Value::as_str) {
        Some("Extensions.koyoInstalledChanged") => list(),
        Some("Extensions.koyoActionUpdated") => action_updated(params),
        _ => {}
    }
}

fn answered(asked: &Asked, message: &Value) {
    let failed = message.get("error").map(|error| {
        error.get("message").and_then(Value::as_str).unwrap_or("no reason given").to_owned()
    });
    match asked {
        Asked::Installed => {
            let again = STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.listing = false;
                std::mem::take(&mut state.stale)
            });
            match failed {
                // An engine built without Koyo's patches has no such command.
                Some(why) => log::warn!("extensions: the engine doesn't list them ({why})"),
                None => installed(message.get("result").unwrap_or(&Value::Null)),
            }
            if again {
                list();
            }
        }
        Asked::Done(what) => {
            if let Some(why) = failed {
                log::warn!("extensions: {what} failed: {why}");
            }
        }
    }
}

/// Every profile's extensions, as Chromium lists them: told to the app a profile at a time,
/// under the name the app knows the profile by. Every profile a pane was opened in is told,
/// one with none too: the app shows where to get some.
fn installed(result: &Value) {
    let mut by_profile: BTreeMap<String, Vec<Extension>> = BTreeMap::new();
    let listed = result.get("extensions").and_then(Value::as_array);
    log::debug!("extensions: the engine lists {}", listed.map_or(0, Vec::len));
    for entry in listed.into_iter().flatten() {
        let folder = entry.get("profile").and_then(Value::as_str).unwrap_or_default();
        // A profile of no project (the engine's own, the developer tools') has no row.
        let Some(profile) = host::profile_of_folder(folder) else {
            continue;
        };
        if let Some(extension) = extension(entry) {
            by_profile.entry(profile).or_default().push(extension);
        }
    }
    for profile in host::profiles() {
        by_profile.entry(profile).or_default();
    }
    for (profile, mut extensions) in by_profile {
        extensions.sort_by(|a, b| {
            (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id))
        });
        host::send(&HostEvent::Extensions { profile, extensions });
    }
}

/// One extension of Chromium's list (`KoyoExtension` in the protocol).
fn extension(entry: &Value) -> Option<Extension> {
    let text = |name: &str| entry.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    let flag = |name: &str| entry.get(name).and_then(Value::as_bool).unwrap_or(false);
    let id = text("id");
    if id.is_empty() {
        return None;
    }
    Some(Extension {
        id,
        name: text("name"),
        version: text("version"),
        description: text("description"),
        enabled: flag("enabled"),
        removable: flag("removable"),
        pinned: flag("pinned"),
        options_url: text("optionsUrl"),
        homepage_url: text("homepageUrl"),
        icon_path: text("iconPath"),
        source: match text("location").as_str() {
            "store" => ExtensionSource::Store,
            "unpacked" | "commandLine" => ExtensionSource::Unpacked,
            "policy" => ExtensionSource::Policy,
            _ => ExtensionSource::Other,
        },
    })
}

/// An extension set its toolbar action, for a page or for every page of a profile.
fn action_updated(params: &Value) {
    let text = |name: &str| params.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    let Some(profile) = host::profile_of_folder(&text("profile")) else {
        return;
    };
    let pane = match params.get("browserId").and_then(Value::as_i64) {
        // A page that isn't a pane's (a window of Chrome's own) has no icon of the app's.
        Some(browser) => match i32::try_from(browser).ok().and_then(host::pane_of_browser) {
            Some(pane) => Some(pane),
            None => return,
        },
        None => None,
    };
    host::send(&HostEvent::ExtensionAction {
        profile,
        extension: text("id"),
        pane,
        action: ExtensionAction {
            icon: text("icon"),
            title: text("title"),
            badge_text: text("badgeText"),
            badge_color: text("badgeColor"),
            badge_text_color: text("badgeTextColor"),
            enabled: params.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        },
    });
}

/// The person is on `pane`'s page: Chromium is told, once for each change, and extensions
/// get it as the current window's active tab. A page whose browser isn't made yet is told
/// when it is ([`browser_created`]).
pub(crate) fn active(pane: PaneId) {
    if !pipe::is_open() {
        return;
    }
    let was = STATE.with(|state| state.borrow_mut().active.replace(pane));
    if was != Some(pane) {
        tell_active(pane);
    }
}

fn tell_active(pane: PaneId) {
    if let Some(browser) = host::browser_id(pane) {
        ask("Extensions.koyoSetActivePage", &json!({ "browserId": browser }), Asked::Done("the active page"));
    }
}

/// `pane`'s browser is made: if it's the page the person is on, or the first there is,
/// Chromium is told so now.
pub(crate) fn browser_created(pane: PaneId) {
    if !pipe::is_open() {
        return;
    }
    let tell = STATE.with(|state| {
        let mut state = state.borrow_mut();
        *state.active.get_or_insert(pane) == pane
    });
    if tell {
        tell_active(pane);
    }
}

/// `pane` closed: it's no longer the page the person is on.
pub(crate) fn pane_closed(pane: PaneId) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.active == Some(pane) {
            state.active = None;
        }
    });
}

/// The page the person is on, for what has to go to some page of the app's.
pub(crate) fn active_pane() -> Option<PaneId> {
    STATE.with(|state| state.borrow().active)
}

/// A press on `extension`'s icon for `pane`'s page: Chrome runs its action there, and a popup
/// it opens hangs from `at` (a point of the screen, from the bottom left).
pub(crate) fn act(pane: PaneId, extension: &str, at: [f64; 2]) {
    let Some(browser) = host::browser_id(pane) else {
        return;
    };
    // The page pressed for is the one the person is on, whatever had the keyboard.
    active(pane);
    popup::expect(pane, at);
    ask(
        "Extensions.koyoTriggerAction",
        &json!({ "id": extension, "browserId": browser }),
        Asked::Done("an action"),
    );
}

/// Turns `extension` of `profile` on or off, pins it, or removes it. Chromium says the list
/// changed once it has, and the app hears how it stands then.
pub(crate) fn change(profile: &str, extension: &str, change: ExtensionChange) {
    let Some(folder) = host::folder_of_profile(profile) else {
        return;
    };
    let mut params = json!({ "id": extension, "profile": folder });
    let (method, what) = match change {
        ExtensionChange::Enabled { on } => {
            params["enabled"] = json!(on);
            ("Extensions.koyoSetEnabled", "turning one on or off")
        }
        ExtensionChange::Pinned { on } => {
            params["pinned"] = json!(on);
            ("Extensions.koyoSetPinned", "pinning one")
        }
        ExtensionChange::Remove => ("Extensions.koyoUninstall", "removing one"),
    };
    ask(method, &params, Asked::Done(what));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extension_is_read_from_chromiums_list() {
        let entry = json!({
            "id": "ddkjiahejlhfcafbddmgiahcphecmpfh", "profile": "profile-proj", "name": "uBlock Origin Lite",
            "version": "2026.1101.1", "description": "Blocks ads.", "enabled": true, "removable": true,
            "hasAction": true, "hasPopup": true, "pinned": false, "optionsUrl": "chrome-extension://d/dashboard.html",
            "homepageUrl": "https://chromewebstore.google.com/detail/d", "iconPath": "/p/Extensions/d/1/img/icon_64.png",
            "location": "store",
        });
        let read = extension(&entry).unwrap();
        assert_eq!(read.name, "uBlock Origin Lite");
        assert!(read.enabled && read.removable && !read.pinned);
        assert_eq!(read.source, ExtensionSource::Store);
        assert_eq!(read.icon_path, "/p/Extensions/d/1/img/icon_64.png");
        // One with no id is nothing to show; what's left out is empty or off.
        assert!(extension(&json!({ "name": "Nameless" })).is_none());
        let bare = extension(&json!({ "id": "a", "location": "commandLine" })).unwrap();
        assert_eq!(bare.source, ExtensionSource::Unpacked);
        assert!(!bare.enabled && bare.options_url.is_empty());
    }
}
