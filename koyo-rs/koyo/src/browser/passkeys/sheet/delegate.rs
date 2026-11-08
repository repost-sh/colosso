//! What the sheet's controller calls: the delegate that hears how the sheet ended, which is
//! also what tells it the window to come up over.

use std::cell::Cell;

use futures::channel::oneshot;
use koyo_browser_ipc::{Bytes, Got, PasskeyAnswer, PasskeyError};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{
    DefinedClass as _, MainThreadMarker, MainThreadOnly, Message, available, define_class, msg_send,
};
use objc2_app_kit::NSWindow;
use objc2_authentication_services::{
    ASAuthorization, ASAuthorizationController, ASAuthorizationControllerDelegate,
    ASAuthorizationControllerPresentationContextProviding, ASAuthorizationErrorDomain,
    ASAuthorizationPlatformPublicKeyCredentialAssertion,
    ASAuthorizationPlatformPublicKeyCredentialRegistration,
    ASAuthorizationPublicKeyCredentialAssertion, ASAuthorizationPublicKeyCredentialAttachment,
    ASAuthorizationPublicKeyCredentialRegistration,
    ASAuthorizationSecurityKeyPublicKeyCredentialAssertion,
    ASAuthorizationSecurityKeyPublicKeyCredentialRegistration, ASPresentationAnchor,
};
use objc2_foundation::NSError;

use super::answer::{self, Asked, Authenticator, CROSS_PLATFORM, PLATFORM, Registration};
use super::requests::transport_name;

/// The delegate's state.
pub(super) struct Ivars {
    /// The window the sheet comes up over.
    window: Retained<NSWindow>,
    /// Where the answer goes, until it's sent or the page stops waiting.
    tell: Cell<Option<oneshot::Sender<PasskeyAnswer>>>,
    /// What the answer takes from the page's ask.
    asked: Asked,
}

define_class!(
    /// Hears how the sheet ended and tells the page; shows the sheet over the page's window.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KoyoPasskeySheet"]
    #[ivars = Ivars]
    pub(super) struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl ASAuthorizationControllerDelegate for Delegate {
        #[unsafe(method(authorizationController:didCompleteWithAuthorization:))]
        fn completed(
            &self,
            _controller: &ASAuthorizationController,
            authorization: &ASAuthorization,
        ) {
            let answer = answer_of(authorization, self.ivars().asked);
            self.tell(answer);
        }

        #[unsafe(method(authorizationController:didCompleteWithError:))]
        fn failed(&self, _controller: &ASAuthorizationController, error: &NSError) {
            let domain = error.domain();
            let code = error.code();
            log::warn!(
                "the passkey sheet ended in {domain} {code}: {}",
                error.localizedDescription()
            );
            // SAFETY: the framework's constant, set as it loads and never changed.
            let ours = *domain == *unsafe { ASAuthorizationErrorDomain };
            self.tell(PasskeyAnswer::Refused(answer::refusal(ours, code)));
        }
    }

    unsafe impl ASAuthorizationControllerPresentationContextProviding for Delegate {
        #[unsafe(method_id(presentationAnchorForAuthorizationController:))]
        fn anchor(
            &self,
            _controller: &ASAuthorizationController,
        ) -> Retained<ASPresentationAnchor> {
            let window = self.ivars().window.clone();
            Retained::into_super(Retained::into_super(window))
        }
    }
);

impl Delegate {
    /// The delegate for a sheet over `window`, telling `tell` how it ended.
    pub(super) fn new(
        main: MainThreadMarker,
        window: &NSWindow,
        tell: oneshot::Sender<PasskeyAnswer>,
        asked: Asked,
    ) -> Retained<Self> {
        let this = Self::alloc(main).set_ivars(Ivars {
            window: Retained::from(window),
            tell: Cell::new(Some(tell)),
            asked,
        });
        // SAFETY: `NSObject`'s `init`, on an instance just allocated with its ivars set.
        unsafe { msg_send![super(this), init] }
    }

    /// Stops telling the page; whether it was still waiting.
    pub(super) fn stop(&self) -> bool {
        self.ivars().tell.take().is_some()
    }

    /// Tells the page `answer`, if it still waits.
    fn tell(&self, answer: PasskeyAnswer) {
        if let Some(tell) = self.ivars().tell.take() {
            // The receiver is gone only with the page's request.
            let _ = tell.send(answer);
        }
    }
}

/// The page's answer from what the sheet ended with.
fn answer_of(authorization: &ASAuthorization, asked: Asked) -> PasskeyAnswer {
    // SAFETY: the authorization the controller completed with, whose credential it made.
    let credential = unsafe { authorization.credential() };
    let credential: &AnyObject = (*credential).as_ref();
    if let Some(made) =
        credential.downcast_ref::<ASAuthorizationPlatformPublicKeyCredentialRegistration>()
    {
        // SAFETY: on macOS 13.5 or later, as every shown sheet is, which has `attachment`.
        let attachment = attachment(unsafe { made.attachment() });
        // Synced with the person's devices, and offered to a phone nearby.
        let transports = vec!["hybrid".to_owned(), "internal".to_owned()];
        let made = registration(made, attachment, transports);
        return answer::created(made, Authenticator::Platform, asked);
    }
    if let Some(made) =
        credential.downcast_ref::<ASAuthorizationSecurityKeyPublicKeyCredentialRegistration>()
    {
        let made = registration(made, CROSS_PLATFORM, key_transports(made));
        return answer::created(made, Authenticator::SecurityKey, asked);
    }
    if let Some(signed) =
        credential.downcast_ref::<ASAuthorizationPlatformPublicKeyCredentialAssertion>()
    {
        // SAFETY: on macOS 13.5 or later, as every shown sheet is, which has `attachment`.
        return sign_in(signed, attachment(unsafe { signed.attachment() }));
    }
    if let Some(signed) =
        credential.downcast_ref::<ASAuthorizationSecurityKeyPublicKeyCredentialAssertion>()
    {
        return sign_in(signed, CROSS_PLATFORM);
    }
    log::warn!(
        "the passkey sheet answered with a {}",
        credential.class().name().to_string_lossy()
    );
    PasskeyAnswer::refused(
        PasskeyError::Unknown,
        "The sheet answered with something else.",
    )
}

/// A new passkey, as either request's credential gives it.
fn registration(
    made: &(impl ASAuthorizationPublicKeyCredentialRegistration + Message),
    attachment: &'static str,
    transports: Vec<String>,
) -> Registration {
    // SAFETY: a credential the system made.
    unsafe {
        Registration {
            id: made.credentialID().to_vec(),
            client_data_json: made.rawClientDataJSON().to_vec(),
            attestation_object: made.rawAttestationObject().map(|object| object.to_vec()),
            attachment,
            transports,
        }
    }
}

/// How a security key's new passkey is reached, as it says from macOS 14.5 on.
fn key_transports(made: &ASAuthorizationSecurityKeyPublicKeyCredentialRegistration) -> Vec<String> {
    if !available!(macos = 14.5) {
        return vec!["usb".to_owned()];
    }
    // SAFETY: macOS 14.5 or later, which has `transports`.
    let transports = unsafe { made.transports() };
    transports
        .iter()
        .map(|transport| transport_name(&transport))
        .collect()
}

/// The page's sign-in, as either request's credential gives it.
fn sign_in(
    signed: &(impl ASAuthorizationPublicKeyCredentialAssertion + Message),
    attachment: &'static str,
) -> PasskeyAnswer {
    // SAFETY: a credential the system made.
    unsafe {
        PasskeyAnswer::Got(Got {
            id: Bytes(signed.credentialID().to_vec()),
            client_data_json: Bytes(signed.rawClientDataJSON().to_vec()),
            authenticator_data: Bytes(signed.rawAuthenticatorData().to_vec()),
            signature: Bytes(signed.signature().to_vec()),
            user_handle: answer::user_handle(signed.userID().to_vec()),
            attachment: Some(attachment.to_owned()),
        })
    }
}

/// `WebAuthn`'s `authenticatorAttachment` for the system's: a phone is cross-platform.
fn attachment(attachment: ASAuthorizationPublicKeyCredentialAttachment) -> &'static str {
    if attachment == ASAuthorizationPublicKeyCredentialAttachment::CrossPlatform {
        CROSS_PLATFORM
    } else {
        PLATFORM
    }
}
