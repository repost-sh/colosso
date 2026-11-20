//! Who answers a page's passkey request in a build without Apple's browser passkey
//! entitlement (`docs/browser.md` 9.3). macOS keeps this Mac's own passkeys (iCloud Keychain,
//! and the person's other providers) from such a build, but not a security key over USB, nor
//! a phone, found by the QR code Chrome shows and reached over Bluetooth: Chromium's own
//! `WebAuthn` answers those with no entitlement, and checks the frame's origin itself. So a
//! request goes to the engine, unless only this Mac's passkeys could answer it, which is
//! turned down at once: for its origin if that's wrong (`origin`), else saying why.
use koyo_browser_ipc::{PasskeyAsk, PasskeyError, Refusal};

/// What a page is told when only this Mac's passkeys could answer it.
pub(super) const MAC_ONLY: &str = "This Mac's passkeys need Koyo to be signed with Apple's \
    browser passkey entitlement, which this build isn't; a security key or a phone can still \
    be used.";

/// Whether the engine's own `WebAuthn` may answer `ask` in a build without the entitlement.
///
/// # Errors
/// [`MAC_ONLY`], as `NotAllowedError`, for a request only this Mac's passkeys could answer:
/// a new passkey asked of the platform's authenticator, or a sign-in with passkeys that
/// live only on a device's own (`internal` their only transport).
pub(super) fn to_engine(ask: &PasskeyAsk) -> Result<(), Refusal> {
    let mac_only = match ask {
        PasskeyAsk::Create(creation) => {
            creation
                .authenticator_selection
                .authenticator_attachment
                .as_deref()
                == Some("platform")
        }
        PasskeyAsk::Get(assertion) => {
            let internal = |transports: &[String]| {
                !transports.is_empty() && transports.iter().all(|t| t == "internal")
            };
            !assertion.allow_credentials.is_empty()
                && (assertion.allow_credentials.iter()).all(|c| internal(&c.transports))
        }
    };
    if mac_only {
        return Err(Refusal::new(PasskeyError::NotAllowed, MAC_ONLY));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAC_ONLY, to_engine};
    use crate::passkeys::asks::read;
    use koyo_browser_ipc::PasskeyError;

    fn creation(attachment: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "rp": {"name": "Example"},
            "user": {"id": "AQ", "name": "ada", "displayName": "Ada"},
            "challenge": "AAECAwQFBgcICQoLDA0ODw",
            "pubKeyCredParams": [{"type": "public-key", "alg": -7}],
            "authenticatorSelection": {"authenticatorAttachment": attachment},
        })
    }

    fn sign_in(transports: &[&[&str]]) -> serde_json::Value {
        let allowed: Vec<serde_json::Value> = (transports.iter())
            .map(|transports| serde_json::json!({"id": "AQID", "transports": transports}))
            .collect();
        serde_json::json!({"challenge": "AA", "allowCredentials": allowed})
    }

    fn routed(op: &str, options: serde_json::Value) -> Result<(), (PasskeyError, String)> {
        let ask = read(op, options).expect("a request a page can send");
        to_engine(&ask).map_err(|refusal| (refusal.error, refusal.message))
    }

    #[test]
    fn a_security_key_or_a_phone_may_answer_goes_to_the_engine() {
        assert_eq!(
            routed("create", creation(None)),
            Ok(()),
            "any authenticator"
        );
        assert_eq!(routed("create", creation(Some("cross-platform"))), Ok(()));
        assert_eq!(
            routed("get", sign_in(&[])),
            Ok(()),
            "a passkey the page doesn't name"
        );
        assert_eq!(
            routed("get", sign_in(&[&["usb", "nfc"]])),
            Ok(()),
            "a security key's"
        );
        assert_eq!(
            routed("get", sign_in(&[&["internal", "hybrid"]])),
            Ok(()),
            "on a phone too"
        );
        assert_eq!(
            routed("get", sign_in(&[&[]])),
            Ok(()),
            "transports the page doesn't know"
        );
        assert_eq!(routed("get", sign_in(&[&["internal"], &["usb"]])), Ok(()));
    }

    #[test]
    fn what_only_this_macs_passkeys_could_answer_is_turned_down_saying_why() {
        let refused = Err((PasskeyError::NotAllowed, MAC_ONLY.to_owned()));
        assert_eq!(routed("create", creation(Some("platform"))), refused);
        assert_eq!(
            routed("get", sign_in(&[&["internal"], &["internal"]])),
            refused
        );
    }
}
