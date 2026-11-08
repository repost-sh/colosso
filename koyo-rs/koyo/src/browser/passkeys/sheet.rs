//! The system's passkey sheet for one request: `ASAuthorizationController` over the page's
//! window, asked with a browser's own requests (`choice`, `requests`), which came in macOS
//! 13.5, security keys' in 14.4. An older system turns the page down, which then signs in
//! another way. The sheet's ending is the page's answer (`delegate`, `answer`).

use futures::channel::oneshot;
use koyo_browser_ipc::{PasskeyAnswer, PasskeyAsk, PasskeyError, Refusal};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AllocAnyThread as _, MainThreadMarker, available};
use objc2_app_kit::NSWindow;
use objc2_authentication_services::{ASAuthorizationController, ASAuthorizationRequest};
use objc2_foundation::NSArray;

use answer::Asked;
use delegate::Delegate;

mod answer;
mod choice;
mod delegate;
mod requests;
#[cfg(test)]
mod tests;

/// The sheet for one request; dropped, it's taken down. The default is none shown.
#[derive(Default)]
pub(super) struct Sheet {
    /// None when the page was answered without one.
    _shown: Option<Shown>,
}

impl Sheet {
    /// Asks macOS for `ask` from a frame of `origin`, the sheet over `window`. The answer
    /// comes once on the receiver, on the main thread.
    pub(super) fn open(
        window: &NSWindow,
        origin: &str,
        ask: &PasskeyAsk,
    ) -> (Self, oneshot::Receiver<PasskeyAnswer>) {
        match requests_for(origin, ask) {
            Ok(requests) => {
                let (tell, told) = oneshot::channel();
                let shown = Shown::perform(window, &requests, Asked::of(ask), tell);
                (
                    Self {
                        _shown: Some(shown),
                    },
                    told,
                )
            }
            Err(refusal) => Self::answered(PasskeyAnswer::Refused(refusal)),
        }
    }

    /// No sheet: the page gets `answer` at once.
    pub(super) fn answered(answer: PasskeyAnswer) -> (Self, oneshot::Receiver<PasskeyAnswer>) {
        let (tell, told) = oneshot::channel();
        // The receiver is right here.
        let _ = tell.send(answer);
        (Self::default(), told)
    }
}

/// The system's requests for `ask`, or why it can't be asked here.
fn requests_for(
    origin: &str,
    ask: &PasskeyAsk,
) -> Result<Vec<Retained<ASAuthorizationRequest>>, Refusal> {
    if !available!(macos = 13.5) {
        return Err(Refusal::new(
            PasskeyError::NotAllowed,
            "Passkeys need macOS 13.5 or later.",
        ));
    }
    let choice = choice::choose(ask, available!(macos = 14.4))?;
    Ok(requests::requests(origin, ask, &choice))
}

/// A sheet macOS was asked to show.
struct Shown {
    controller: Retained<ASAuthorizationController>,
    /// The controller's delegate and presentation context provider, which it holds weakly.
    delegate: Retained<Delegate>,
}

impl Shown {
    /// Asks macOS for `requests`, over `window`.
    fn perform(
        window: &NSWindow,
        requests: &[Retained<ASAuthorizationRequest>],
        asked: Asked,
        tell: oneshot::Sender<PasskeyAnswer>,
    ) -> Self {
        let delegate = Delegate::new(MainThreadMarker::from(window), window, tell, asked);
        // SAFETY: requests made for this system (`requests_for`), on the main thread, as
        // `window` says. The controller holds its delegate and provider weakly, and `Shown`
        // keeps them for as long as it keeps the controller.
        let controller = unsafe {
            let controller = ASAuthorizationController::initWithAuthorizationRequests(
                ASAuthorizationController::alloc(),
                &NSArray::from_retained_slice(requests),
            );
            controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            controller.setPresentationContextProvider(Some(ProtocolObject::from_ref(&*delegate)));
            controller.performRequests();
            controller
        };
        Self {
            controller,
            delegate,
        }
    }
}

impl Drop for Shown {
    /// The page stopped waiting before the sheet ended: it comes down, and with the delegate
    /// gone the controller tells nobody.
    fn drop(&mut self) {
        // `cancel` came in macOS 13.0, before any sheet is shown.
        if self.delegate.stop() && available!(macos = 13.0) {
            // SAFETY: the controller performing the sheet's requests, on the main thread.
            unsafe { self.controller.cancel() };
        }
    }
}
