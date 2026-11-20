//! Passkeys for the pages: `WebAuthn`'s `navigator.credentials.create` and `get`, taken from
//! every frame by a bridge each renderer runs before the page's scripts (`renderer`), checked
//! here against the frame's origin as the engine knows it (`origin`, `asks`), and asked of
//! the app ([`HostEvent::Passkey`]), which asks macOS in the system's passkey sheet. Its
//! answer goes back to the frame in `WebAuthn`'s JSON forms (`answers`), which the bridge
//! turns into the `PublicKeyCredential` the page gets.
//!
//! Until the app says it can ask for passkeys ([`koyo_browser_ipc::Hello::passkeys`]), as it
//! can only once signed with Apple's browser passkey entitlement, a request goes to Chromium's
//! own `WebAuthn` instead, which reaches a security key and a phone without it, its dialog
//! over the page as each window Chromium shows for a page is (`native/holder.rs`); one only
//! this Mac's passkeys could answer is turned down at once with `NotAllowedError`, saying why
//! (`route`). The availability checks are the engine's then.
use crate::pending::Pending;
use cef::{Browser, CefString, Frame, ImplBrowser, ImplFrame, ImplListValue, ImplProcessMessage};
use cef::{ProcessId, ProcessMessage, process_message_create};
use koyo_browser_ipc::{HostEvent, PaneId, PasskeyAnswer, Refusal};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

mod answers;
mod asks;
mod attestation;
mod origin;
pub(crate) mod renderer;
mod route;

/// The process messages' name, between a renderer's bridge and the browser process.
const MESSAGE: &str = "koyo-passkey";

/// Whether the app can ask for passkeys, as it said in its hello.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Says whether the app can ask for passkeys; until it does, it can't.
pub(crate) fn enable(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// A request waiting on the app.
struct Waiting {
    /// The frame that asked, which the answer goes to.
    frame: Frame,
    frame_id: String,
    /// The renderer's number for the request.
    id: u64,
}

thread_local! {
    static REQUESTS: RefCell<Pending<Waiting>> = const { RefCell::new(Pending::new()) };
}

fn text(value: &cef::CefStringUserfree) -> String {
    CefString::from(value).to_string()
}

/// Sends the bridge's message `said` to `frame`'s side in process `to`.
fn send(frame: &Frame, to: ProcessId, said: &serde_json::Value) {
    let Some(mut message) = process_message_create(Some(&CefString::from(MESSAGE))) else {
        return;
    };
    if let Some(args) = message.argument_list() {
        args.set_string(0, Some(&CefString::from(said.to_string().as_str())));
    }
    frame.send_process_message(to, Some(&mut message));
}

/// What a process message of the bridge's says, as JSON text; `None` for another's.
fn said(message: &ProcessMessage) -> Option<String> {
    if text(&message.name()) != MESSAGE {
        return None;
    }
    Some(
        message
            .argument_list()
            .map(|args| text(&args.string(0)))
            .unwrap_or_default(),
    )
}

/// What a page's bridge asks.
#[derive(serde::Deserialize)]
struct Asked {
    /// The renderer's number for the request.
    id: u64,
    /// `create`, `get`, `available`, `capabilities` or `cancel`.
    op: String,
    #[serde(default)]
    options: serde_json::Value,
}

/// A process message from a page's renderer, from `frame` of `browser`, the browser of
/// `pane`: `true` when it was the bridge's.
pub(crate) fn from_renderer(
    pane: PaneId,
    browser: &Browser,
    frame: &Frame,
    message: &ProcessMessage,
) -> bool {
    let Some(said) = said(message) else {
        return false;
    };
    let Ok(Asked { id, op, options }) = serde_json::from_str(&said) else {
        return true;
    };
    match op.as_str() {
        "available" | "capabilities" if !enabled() => to_engine(frame, id),
        "available" => reply(frame, id, Ok(serde_json::Value::Bool(true))),
        "capabilities" => reply(frame, id, Ok(answers::capabilities(true))),
        "cancel" => {
            let frame_id = text(&frame.identifier());
            withdraw(pane, |w| w.id == id && w.frame_id == frame_id);
        }
        "create" | "get" => ask(pane, browser, frame, id, &op, options),
        _ => {}
    }
    true
}

/// A page's `create` or `get`: checked, then asked of the app, or turned down.
fn ask(
    pane: PaneId,
    browser: &Browser,
    frame: &Frame,
    id: u64,
    op: &str,
    options: serde_json::Value,
) {
    let address = text(&frame.url());
    let mut ancestors = Vec::new();
    let mut above = frame.parent();
    while let Some(parent) = above {
        ancestors.push(text(&parent.url()));
        above = parent.parent();
    }
    if frame.is_main() != 1
        && ancestors.is_empty()
        && let Some(main) = browser.main_frame()
    {
        ancestors.push(text(&main.url()));
    }
    let asked = asks::read(op, options);
    if !enabled() {
        let routed = asked.and_then(|ask| match route::to_engine(&ask) {
            Ok(()) => Ok(()),
            Err(mac_only) => asks::checked(ask, &address, &ancestors).and(Err(mac_only)),
        });
        match routed {
            Ok(()) => to_engine(frame, id),
            Err(refusal) => reply(frame, id, Err(refusal)),
        }
        return;
    }
    let checked = asked.and_then(|ask| asks::checked(ask, &address, &ancestors));
    let (origin, ask) = match checked {
        Ok(checked) => checked,
        Err(refusal) => {
            reply(frame, id, Err(refusal));
            return;
        }
    };
    let waiting = Waiting {
        frame: frame.clone(),
        frame_id: text(&frame.identifier()),
        id,
    };
    let request = REQUESTS.with(|requests| requests.borrow_mut().add(pane, waiting));
    crate::host::send(&HostEvent::Passkey {
        pane,
        request,
        origin,
        ask: Box::new(ask),
    });
}

/// The app's answer to `request`, for the frame that asked.
pub(crate) fn answered(pane: PaneId, request: u64, answer: &PasskeyAnswer) {
    let waiting = REQUESTS.with(|requests| requests.borrow_mut().remove(pane, request));
    if let Some(waiting) = waiting {
        reply(&waiting.frame, waiting.id, answers::shaped(answer));
    }
}

/// `pane`'s page is gone, or its renderer: its requests are withdrawn.
pub(crate) fn pane_gone(pane: PaneId) {
    withdraw(pane, |_| true);
}

/// A document started loading in a frame of `pane`'s `browser`: a new one in the main frame
/// means the old one's frames are gone.
pub(crate) fn load_started(pane: PaneId, browser: &Browser, frame: &Frame) {
    if frame.is_main() == 1 {
        frames_changed(pane, browser, None);
    }
}

/// A frame of `pane`'s `browser` went away (`gone`), its main frame changed, or a new document
/// started in it: the requests of the frames gone are withdrawn. A frame's own frames go with
/// it unannounced, and their renderer's word of it never comes, so every request whose frame
/// isn't the browser's any more goes too; by the time the new document starts, only its
/// frames are.
pub(crate) fn frames_changed(pane: PaneId, browser: &Browser, gone: Option<&Frame>) {
    if !REQUESTS.with(|requests| requests.borrow().has(pane)) {
        return;
    }
    let gone = gone.map(|frame| text(&frame.identifier()));
    let mut list = cef::CefStringList::new();
    browser.frame_identifiers(Some(&mut list));
    let mut current: Vec<String> = list.into_iter().collect();
    current.retain(|frame| gone.as_ref() != Some(frame));
    withdraw(pane, |w| {
        gone.as_ref() == Some(&w.frame_id)
            || w.frame.is_valid() != 1
            || !current.contains(&w.frame_id)
    });
}

/// Withdraws the requests of `pane` that `which` picks, telling the app.
fn withdraw(pane: PaneId, which: impl Fn(&Waiting) -> bool) {
    let gone = REQUESTS.with(|requests| requests.borrow_mut().take(|p, w| p == pane && which(w)));
    for (request, _) in gone {
        crate::host::send(&HostEvent::PasskeyWithdrawn { pane, request });
    }
}

/// Has the engine's own `WebAuthn` answer the bridge's request `id` in `frame`: `{id, engine}`.
fn to_engine(frame: &Frame, id: u64) {
    if frame.is_valid() == 1 {
        send(frame, ProcessId::RENDERER, &serde_json::json!({"id": id, "engine": true}));
    }
}

/// Answers the bridge's request `id` in `frame`: `{id, ok}` or `{id, error}`.
fn reply(frame: &Frame, id: u64, result: Result<serde_json::Value, Refusal>) {
    if frame.is_valid() != 1 {
        return;
    }
    let said = match result {
        Ok(ok) => serde_json::json!({"id": id, "ok": ok}),
        Err(refusal) => serde_json::json!({
            "id": id,
            "error": {"name": refusal.error.name(), "message": refusal.message},
        }),
    };
    send(frame, ProcessId::RENDERER, &said);
}
