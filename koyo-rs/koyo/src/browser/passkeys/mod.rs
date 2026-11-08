//! Passkeys for the pages: a page's `navigator.credentials.create` or `get` reaches the app
//! as a request the host has checked against the frame's origin ([`HostEvent::Passkey`]),
//! and the app asks macOS for it in the system's passkey sheet over the page's window
//! (`sheet`), which offers iCloud Keychain, the person's other credential providers, a phone
//! by QR code and security keys.
//!
//! macOS answers a browser's passkey requests for any site only when the app is signed with
//! Apple's browser-passkey entitlement ([`ENTITLEMENT`]). Without it the app tells the host
//! so as it starts it, and the host turns every request down at once, so a page goes
//! straight to its other ways of signing in (`docs/browser.md` says how Koyo gets signed for
//! it).
//!
//! A page has one request at a time; it waits until its pane is drawn, so the sheet comes up
//! over the window that shows the page.
//!
//! [`HostEvent::Passkey`]: koyo_browser_ipc::HostEvent::Passkey

use std::sync::OnceLock;

use futures::channel::oneshot;
use gpui_kit::{Context, Task, Window};
use koyo_browser_ipc::{AppRequest, PasskeyAnswer, PasskeyAsk, PasskeyError};

use super::view::BrowserView;

mod sheet;

/// The managed entitlement macOS asks of a browser before its passkey sheet takes requests
/// for any site.
const ENTITLEMENT: &str = "com.apple.developer.web-browser.public-key-credential";

/// Whether this app is signed with [`ENTITLEMENT`], read once off its own task's signature.
pub fn entitled() -> bool {
    static ENTITLED: OnceLock<bool> = OnceLock::new();
    *ENTITLED.get_or_init(|| {
        let entitled = has_entitlement(ENTITLEMENT);
        if !entitled {
            log::info!(
                "passkeys are turned down: the app isn't signed with {ENTITLEMENT} (docs/browser.md)"
            );
        }
        entitled
    })
}

/// Whether this process's signature carries entitlement `name` as `true`.
fn has_entitlement(name: &str) -> bool {
    use objc2_core_foundation::{CFBoolean, CFString};
    use objc2_security::SecTask;
    // SAFETY: the default allocator; the task is this process's own.
    let Some(task) = (unsafe { SecTask::from_self(None) }) else {
        return false;
    };
    let name = CFString::from_str(name);
    // SAFETY: a valid task and name; no error is asked for, which leaves the pointer null.
    let value = unsafe { task.value_for_entitlement(&name, std::ptr::null_mut()) };
    value
        .and_then(|value| value.downcast::<CFBoolean>().ok())
        .is_some_and(|value| value.as_bool())
}

/// A test's stand-in for the system's sheet, answering each request from the page's origin
/// and what it asks, or leaving it unanswered (`None`), as a person who hasn't picked yet;
/// so a page's passkeys can be tested end to end without the entitlement.
#[cfg(test)]
pub struct TestSheet(pub fn(&str, &PasskeyAsk) -> Option<PasskeyAnswer>);
#[cfg(test)]
impl gpui_kit::Global for TestSheet {}

/// A page's passkey request, from the host's asking to the app's answer.
#[derive(Default)]
pub(super) struct Passkey {
    asked: Option<Asked>,
}

#[cfg(test)]
impl Passkey {
    /// Whether a request waits on the sheet.
    pub(super) fn waiting(&self) -> bool {
        self.asked.is_some()
    }
}

struct Asked {
    request: u64,
    origin: String,
    ask: PasskeyAsk,
    /// Once the pane was drawn: the sheet on screen, which goes when dropped, and what hears
    /// its answer and sends it to the page.
    opened: Option<(sheet::Sheet, Task<()>)>,
}

impl BrowserView {
    /// The page asks for a passkey; the sheet comes up once the pane is drawn. A page asking
    /// while it waits on one is turned down, as in Chrome.
    pub(in crate::browser) fn passkey_asked(
        &mut self,
        request: u64,
        origin: String,
        ask: PasskeyAsk,
        cx: &mut Context<Self>,
    ) {
        if self.passkey.asked.is_some() {
            let busy =
                PasskeyAnswer::refused(PasskeyError::NotAllowed, "A request is already pending.");
            self.send_passkey(request, busy, cx);
            return;
        }
        self.passkey.asked = Some(Asked {
            request,
            origin,
            ask,
            opened: None,
        });
        cx.notify();
    }

    /// The page gave up on `request`: its sheet goes, and no answer is sent.
    pub(in crate::browser) fn passkey_withdrawn(&mut self, request: u64) {
        self.passkey.asked.take_if(|asked| asked.request == request);
    }

    /// The page that asked is gone, or the host that sent it: its sheet goes, unanswered.
    pub(super) fn forget_passkey(&mut self) {
        self.passkey.asked = None;
    }

    /// Brings up the sheet for a request waiting on it, over `window`.
    pub(super) fn ask_passkey(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asked) = self.passkey.asked.as_mut() else {
            return;
        };
        if asked.opened.is_some() {
            return;
        }
        let request = asked.request;
        let (sheet, answer) = open(&asked.origin, &asked.ask, window, cx);
        let answering = cx.spawn(async move |view, cx| {
            let answer = answer.await.unwrap_or_else(|_| {
                PasskeyAnswer::refused(PasskeyError::NotAllowed, "The request was cancelled.")
            });
            let _ = view.update(cx, |view, cx| {
                // Withdrawn meanwhile: the page isn't waiting any more.
                if view
                    .passkey
                    .asked
                    .take_if(|asked| asked.request == request)
                    .is_some()
                {
                    view.send_passkey(request, answer, cx);
                }
            });
        });
        asked.opened = Some((sheet, answering));
    }

    fn send_passkey(&self, request: u64, answer: PasskeyAnswer, cx: &mut Context<Self>) {
        let pane = self.pane;
        self.send_to_page(
            AppRequest::Passkey {
                pane,
                request,
                answer,
            },
            cx,
        );
    }
}

/// The sheet for `ask` over `window`, and where its answer comes: at once, when there's no
/// sheet to show.
fn open(
    origin: &str,
    ask: &PasskeyAsk,
    window: &Window,
    cx: &mut Context<BrowserView>,
) -> (sheet::Sheet, oneshot::Receiver<PasskeyAnswer>) {
    #[cfg(test)]
    if let Some(TestSheet(answer)) = cx.try_global::<TestSheet>() {
        if let Some(answer) = answer(origin, ask) {
            return sheet::Sheet::answered(answer);
        }
        // Never answered, as a sheet left open.
        let (tell, told) = oneshot::channel();
        std::mem::forget(tell);
        return (sheet::Sheet::default(), told);
    }
    let _ = cx;
    match crate::native::window::ns_window(window) {
        Some(ns_window) => sheet::Sheet::open(&ns_window, origin, ask),
        None => sheet::Sheet::answered(PasskeyAnswer::refused(
            PasskeyError::NotAllowed,
            "There's no window to ask in.",
        )),
    }
}
