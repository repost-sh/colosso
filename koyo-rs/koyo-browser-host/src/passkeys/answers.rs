//! The app's answers, for the frame that asked: a new passkey or a sign-in in `WebAuthn`'s
//! JSON forms (`RegistrationResponseJSON`, `AuthenticationResponseJSON`), which the bridge
//! makes the page's `PublicKeyCredential` of and gives back from its `toJSON`; and what this
//! browser can do, for `getClientCapabilities`. A new passkey's authenticator data and key
//! are read out of its attestation object here (`attestation`), as the app gives only what
//! the authenticator made.
use super::attestation;
use koyo_browser_ipc::{Bytes, PasskeyAnswer, PasskeyError, Refusal};
use serde_json::{Map, Value, json};

/// `answer` as the bridge takes it.
///
/// # Errors
/// The refusal, when there's no passkey, or a new one's key can't be read.
pub(super) fn shaped(answer: &PasskeyAnswer) -> Result<Value, Refusal> {
    let (id, attachment, extensions, response) = match answer {
        PasskeyAnswer::Refused(refusal) => return Err(refusal.clone()),
        PasskeyAnswer::Created(created) => {
            let Some(attested) = attestation::read(&created.attestation_object.0) else {
                return Err(Refusal::new(
                    PasskeyError::Unknown,
                    "The new passkey's key couldn't be read.",
                ));
            };
            let mut response = json!({
                "clientDataJSON": created.client_data_json,
                "authenticatorData": Bytes(attested.authenticator_data),
                "transports": created.transports,
                "publicKeyAlgorithm": attested.algorithm,
                "attestationObject": created.attestation_object,
            });
            if let Some(key) = attested.public_key {
                response["publicKey"] = json!(Bytes(key));
            }
            let mut extensions = Map::new();
            if let Some(discoverable) = created.discoverable {
                extensions.insert("credProps".into(), json!({"rk": discoverable}));
            }
            (&created.id, &created.attachment, extensions, response)
        }
        PasskeyAnswer::Got(got) => {
            let mut response = json!({
                "clientDataJSON": got.client_data_json,
                "authenticatorData": got.authenticator_data,
                "signature": got.signature,
            });
            if let Some(handle) = &got.user_handle {
                response["userHandle"] = json!(handle);
            }
            (&got.id, &got.attachment, Map::new(), response)
        }
    };
    let mut credential = json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": response,
        "clientExtensionResults": extensions,
    });
    if let Some(attachment) = attachment {
        credential["authenticatorAttachment"] = json!(attachment);
    }
    Ok(credential)
}

/// What `PublicKeyCredential.getClientCapabilities()` says: the system's sheet offers
/// passkeys on this Mac and others synced to it, and a phone, once the app may ask for them;
/// no autofill, related origins or signals.
pub(super) fn capabilities(enabled: bool) -> Value {
    json!({
        "conditionalCreate": false,
        "conditionalGet": false,
        "hybridTransport": enabled,
        "passkeyPlatformAuthenticator": enabled,
        "userVerifyingPlatformAuthenticator": enabled,
        "relatedOrigins": false,
        "signalAllAcceptedCredentials": false,
        "signalCurrentUserDetails": false,
        "signalUnknownCredential": false,
        "extension:credProps": enabled,
    })
}

#[cfg(test)]
mod tests {
    use super::super::attestation::{self, tests::p256_attestation_object};
    use super::{capabilities, shaped};
    use koyo_browser_ipc::{Bytes, Created, Got, PasskeyAnswer, PasskeyError, Refusal};

    fn created(attestation_object: Vec<u8>) -> PasskeyAnswer {
        PasskeyAnswer::Created(Created {
            id: Bytes(vec![0xfb, 0xff]),
            client_data_json: Bytes(b"{}".to_vec()),
            attestation_object: Bytes(attestation_object),
            transports: vec!["hybrid".into(), "internal".into()],
            attachment: Some("platform".into()),
            discoverable: Some(true),
        })
    }

    #[test]
    fn a_new_passkey_is_registration_response_json_with_its_key_read_out() {
        let object = p256_attestation_object();
        let attested = attestation::read(&object).expect("a key");
        let shown = |bytes: Vec<u8>| serde_json::to_value(Bytes(bytes)).expect("json");
        assert_eq!(
            shaped(&created(object.clone())).expect("a credential"),
            serde_json::json!({
                "id": "-_8",
                "rawId": "-_8",
                "type": "public-key",
                "authenticatorAttachment": "platform",
                "clientExtensionResults": {"credProps": {"rk": true}},
                "response": {
                    "clientDataJSON": "e30",
                    "authenticatorData": shown(attested.authenticator_data),
                    "transports": ["hybrid", "internal"],
                    "publicKey": shown(attested.public_key.expect("a P-256 key")),
                    "publicKeyAlgorithm": -7,
                    "attestationObject": shown(object),
                },
            })
        );
    }

    #[test]
    fn a_new_passkey_whose_key_cant_be_read_is_an_unknown_error() {
        let refused = shaped(&created(vec![0xa0])).map_err(|refusal| refusal.error);
        assert_eq!(refused, Err(PasskeyError::Unknown));
    }

    #[test]
    fn a_sign_in_is_authentication_response_json() {
        let got = PasskeyAnswer::Got(Got {
            id: Bytes(vec![1]),
            client_data_json: Bytes(b"{}".to_vec()),
            authenticator_data: Bytes(vec![2]),
            signature: Bytes(vec![3]),
            user_handle: None,
            attachment: None,
        });
        assert_eq!(
            shaped(&got).expect("a credential"),
            serde_json::json!({
                "id": "AQ",
                "rawId": "AQ",
                "type": "public-key",
                "clientExtensionResults": {},
                "response": {"clientDataJSON": "e30", "authenticatorData": "Ag", "signature": "Aw"},
            })
        );
    }

    #[test]
    fn a_refusal_stays_one() {
        let refusal = Refusal::new(PasskeyError::NotAllowed, "cancelled");
        assert_eq!(
            shaped(&PasskeyAnswer::Refused(refusal.clone())),
            Err(refusal)
        );
    }

    #[test]
    fn nothing_is_offered_until_passkeys_are() {
        assert_eq!(capabilities(false)["hybridTransport"], false);
        assert_eq!(capabilities(false)["passkeyPlatformAuthenticator"], false);
        assert_eq!(
            capabilities(true)["userVerifyingPlatformAuthenticator"],
            true
        );
        assert_eq!(capabilities(true)["conditionalGet"], false);
    }
}
