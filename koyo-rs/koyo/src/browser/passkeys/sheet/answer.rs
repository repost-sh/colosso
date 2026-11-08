//! How the sheet ended, as the page's answer: a new passkey, a sign-in, or the error a site
//! takes as "try another way".

use koyo_browser_ipc::{Bytes, Created, PasskeyAnswer, PasskeyAsk, PasskeyError, Refusal};
use objc2_authentication_services::ASAuthorizationError;

/// `authenticatorAttachment` of a passkey on this Mac or in the person's providers.
pub(super) const PLATFORM: &str = "platform";
/// `authenticatorAttachment` of a security key or a phone.
pub(super) const CROSS_PLATFORM: &str = "cross-platform";

/// Which of the requests the person answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Authenticator {
    /// The platform request: their passkeys, or a phone.
    Platform,
    /// The security-key request.
    SecurityKey,
}

/// What the answer to a new passkey takes from the page's ask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Asked {
    /// Whether the page wants to know if the passkey is discoverable (`credProps`).
    pub cred_props: bool,
    /// Whether the page required a discoverable passkey (`residentKey: "required"`).
    pub resident_key_required: bool,
}

impl Asked {
    /// What the answer to `ask` takes from it.
    pub fn of(ask: &PasskeyAsk) -> Self {
        match ask {
            PasskeyAsk::Create(creation) => Self {
                cred_props: creation.extensions.cred_props,
                resident_key_required: creation.authenticator_selection.resident_key.as_deref()
                    == Some("required"),
            },
            PasskeyAsk::Get(_) => Self::default(),
        }
    }
}

/// A new passkey, as the system gave it.
#[derive(Debug)]
pub(super) struct Registration {
    /// The credential's id.
    pub id: Vec<u8>,
    /// The client data the authenticator signed.
    pub client_data_json: Vec<u8>,
    /// None when the system gave no attestation object, which it should always give.
    pub attestation_object: Option<Vec<u8>>,
    /// [`PLATFORM`] or [`CROSS_PLATFORM`].
    pub attachment: &'static str,
    /// How the passkey can be reached from now on.
    pub transports: Vec<String>,
}

/// The page's new passkey, made by `by`. The host reads its authenticator data and key out of
/// the attestation object.
pub(super) fn created(made: Registration, by: Authenticator, asked: Asked) -> PasskeyAnswer {
    let Some(attestation_object) = made.attestation_object else {
        log::warn!("the passkey sheet made a passkey without an attestation object");
        return PasskeyAnswer::refused(
            PasskeyError::Unknown,
            "The new passkey came without its key.",
        );
    };
    // A platform passkey is always discoverable; a security key's only surely when required.
    let discoverable = (asked.cred_props
        && (by == Authenticator::Platform || asked.resident_key_required))
        .then_some(true);
    PasskeyAnswer::Created(Created {
        id: Bytes(made.id),
        client_data_json: Bytes(made.client_data_json),
        attestation_object: Bytes(attestation_object),
        transports: made.transports,
        attachment: Some(made.attachment.to_owned()),
        discoverable,
    })
}

/// A sign-in's user handle from the system's user id, which is empty for a passkey that has
/// none (a security key's, made non-discoverable).
pub(super) fn user_handle(user_id: Vec<u8>) -> Option<Bytes> {
    (!user_id.is_empty()).then_some(Bytes(user_id))
}

/// What the page is told of the sheet's error `code`, `ours` when it's of the sheet's own
/// domain. Every way the person or the system turned the request down is `NotAllowedError`,
/// which sites take as "try another way"; only a passkey the site excluded says more.
pub(super) fn refusal(ours: bool, code: isize) -> Refusal {
    if !ours {
        return Refusal::new(PasskeyError::NotAllowed, "The request wasn't allowed.");
    }
    let (error, message) = match ASAuthorizationError(code) {
        ASAuthorizationError::Canceled => (PasskeyError::NotAllowed, "The request was cancelled."),
        ASAuthorizationError::NotHandled => (
            PasskeyError::NotAllowed,
            "No passkey could answer the request.",
        ),
        ASAuthorizationError::Failed => (PasskeyError::NotAllowed, "The request failed."),
        ASAuthorizationError::NotInteractive => (
            PasskeyError::NotAllowed,
            "The request needed the person, and couldn't ask.",
        ),
        ASAuthorizationError::MatchedExcludedCredential => (
            PasskeyError::InvalidState,
            "The authenticator already holds a passkey for this account.",
        ),
        _ => (PasskeyError::NotAllowed, "The request wasn't allowed."),
    };
    Refusal::new(error, message)
}
