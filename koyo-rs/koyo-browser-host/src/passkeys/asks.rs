//! A page's passkey options, as its bridge sent them: read into the request the app is
//! asked (what a page can't send is a `TypeError`, as the engine's own bindings would throw),
//! and checked against the frame's origin, which fills in the relying party's id.
use super::origin;
use koyo_browser_ipc::{
    Assertion, Creation, CredentialParameters, PasskeyAsk, PasskeyError, Refusal,
};

/// The key types `WebAuthn` takes when a page names none: ES256, then RS256.
const DEFAULT_ALGORITHMS: [i64; 2] = [-7, -257];

/// The request `op` (`create` or `get`) with `options`.
///
/// # Errors
/// A `TypeError` for options a page couldn't have passed.
pub(super) fn read(op: &str, options: serde_json::Value) -> Result<PasskeyAsk, Refusal> {
    let malformed = |error: serde_json::Error| {
        Refusal::new(
            PasskeyError::Type,
            format!("The passkey options are malformed: {error}."),
        )
    };
    match op {
        "create" => {
            let mut creation: Creation = serde_json::from_value(options).map_err(malformed)?;
            if !(1..=64).contains(&creation.user.id.0.len()) {
                return Err(Refusal::new(
                    PasskeyError::Type,
                    "The user's id must be between 1 and 64 bytes.",
                ));
            }
            if creation.pub_key_cred_params.is_empty() {
                creation.pub_key_cred_params = DEFAULT_ALGORITHMS
                    .map(|alg| CredentialParameters { alg })
                    .to_vec();
            }
            Ok(PasskeyAsk::Create(creation))
        }
        "get" => {
            let assertion: Assertion = serde_json::from_value(options).map_err(malformed)?;
            Ok(PasskeyAsk::Get(assertion))
        }
        _ => Err(Refusal::new(
            PasskeyError::NotSupported,
            "Not a passkey request.",
        )),
    }
}

/// `ask` from the frame at `address`, under frames at `ancestors`, checked: the frame's
/// origin and the request, its relying party's id filled in.
///
/// # Errors
/// The refusal [`origin::check`] makes.
pub(super) fn checked(
    mut ask: PasskeyAsk,
    address: &str,
    ancestors: &[String],
) -> Result<(String, PasskeyAsk), Refusal> {
    let allowed = origin::check(address, ancestors, ask.rp_id())?;
    match &mut ask {
        PasskeyAsk::Create(creation) => creation.rp.id = allowed.rp_id,
        PasskeyAsk::Get(assertion) => assertion.rp_id = allowed.rp_id,
    }
    Ok((allowed.origin, ask))
}

#[cfg(test)]
mod tests {
    use super::{checked, read};
    use koyo_browser_ipc::{PasskeyAsk, PasskeyError};

    fn creation(user_id: &str, params: &serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "rp": {"name": "Example"},
            "user": {"id": user_id, "name": "ada", "displayName": "Ada"},
            "challenge": "AAECAwQFBgcICQoLDA0ODw",
            "pubKeyCredParams": params,
        })
    }

    #[test]
    fn a_creation_without_key_types_takes_webauthns_defaults() {
        let Ok(PasskeyAsk::Create(made)) = read("create", creation("AQ", &serde_json::json!([])))
        else {
            panic!("a creation");
        };
        let algorithms: Vec<i64> = made.pub_key_cred_params.iter().map(|p| p.alg).collect();
        assert_eq!(algorithms, [-7, -257]);
    }

    #[test]
    fn a_user_id_of_no_bytes_or_too_many_is_a_type_error() {
        let long = "A".repeat(88);
        for id in ["", long.as_str()] {
            let refused = read("create", creation(id, &serde_json::json!([]))).map(|_| ());
            assert_eq!(
                refused.map_err(|r| r.error),
                Err(PasskeyError::Type),
                "{id}"
            );
        }
    }

    #[test]
    fn malformed_options_are_a_type_error() {
        for (op, options) in [
            ("create", serde_json::json!({"rp": {}})),
            ("get", serde_json::json!({"challenge": 5})),
            ("get", serde_json::Value::Null),
        ] {
            let refused = read(op, options).map(|_| ());
            assert_eq!(refused.map_err(|r| r.error), Err(PasskeyError::Type));
        }
    }

    #[test]
    fn the_checked_request_names_the_frames_origin_and_its_relying_party() {
        let get = read(
            "get",
            serde_json::json!({"challenge": "AA", "rpId": "google.com"}),
        )
        .expect("a sign-in");
        let (origin, ask) =
            checked(get, "https://accounts.google.com/v3/signin", &[]).expect("allowed");
        assert_eq!(origin, "https://accounts.google.com");
        assert_eq!(ask.rp_id(), "google.com");
        let made = read("create", creation("AQ", &serde_json::json!([]))).expect("a creation");
        let (_, ask) = checked(made, "https://www.example.com/", &[]).expect("allowed");
        assert_eq!(ask.rp_id(), "www.example.com");
    }
}
