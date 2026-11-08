use crate::{
    AppRequest, Assertion, Bytes, Created, Creation, HostEvent, PasskeyAnswer, PasskeyAsk,
    PasskeyError, read_frame, write_frame,
};

fn round_trip<T>(message: &T) -> T
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de>,
{
    let mut buf = Vec::new();
    write_frame(&mut buf, message).expect("write");
    read_frame::<T>(&mut buf.as_slice())
        .expect("read")
        .expect("a frame")
}

#[test]
fn bytes_travel_as_base64url_and_take_padding_too() {
    let bytes = Bytes(vec![0xfb, 0xff, 0x01]);
    assert_eq!(serde_json::to_value(&bytes).expect("json"), "-_8B");
    let padded: Bytes = serde_json::from_value("-_8B".into()).expect("unpadded");
    assert_eq!(padded, bytes);
    let padded: Bytes = serde_json::from_value("AQ==".into()).expect("padded");
    assert_eq!(padded.0, [1]);
    assert!(serde_json::from_value::<Bytes>("not base64!".into()).is_err());
}

#[test]
fn a_pages_creation_options_read_as_webauthns_json_form() {
    // What the bridge sends for a page's `create`, as `toJSON` would write the options.
    let ask: PasskeyAsk = serde_json::from_value(serde_json::json!({
        "ceremony": "create",
        "rp": {"id": "example.com", "name": "Example"},
        "user": {"id": "AQID", "name": "ada@example.com", "displayName": "Ada"},
        "challenge": "Y2hhbGxlbmdl",
        "pubKeyCredParams": [{"type": "public-key", "alg": -7}, {"type": "public-key", "alg": -257}],
        "excludeCredentials": [{"type": "public-key", "id": "AAE", "transports": ["internal"]}],
        "authenticatorSelection": {"residentKey": "required", "userVerification": "preferred"},
        "attestation": "none",
        "extensions": {"credProps": true},
    }))
    .expect("creation options");
    let PasskeyAsk::Create(creation) = &ask else {
        panic!("a creation: {ask:?}");
    };
    assert_eq!(ask.rp_id(), "example.com");
    assert_eq!(creation.challenge.0, b"challenge");
    assert_eq!(creation.user.id.0, [1, 2, 3]);
    assert_eq!(creation.user.display_name, "Ada");
    assert_eq!(
        creation
            .pub_key_cred_params
            .iter()
            .map(|p| p.alg)
            .collect::<Vec<_>>(),
        [-7, -257]
    );
    assert_eq!(creation.exclude_credentials[0].id.0, [0, 1]);
    assert_eq!(
        creation.authenticator_selection.resident_key.as_deref(),
        Some("required")
    );
    assert!(creation.extensions.cred_props);
    assert_eq!(round_trip(&ask), ask);
}

#[test]
fn passkey_messages_round_trip() {
    let get = PasskeyAsk::Get(Assertion {
        challenge: Bytes(vec![9; 32]),
        rp_id: "accounts.example.com".into(),
        allow_credentials: Vec::new(),
        user_verification: Some("required".into()),
    });
    let asked = HostEvent::Passkey {
        pane: 4,
        request: 17,
        origin: "https://accounts.example.com".into(),
        ask: Box::new(get),
    };
    assert_eq!(round_trip(&asked), asked);
    let withdrawn = HostEvent::PasskeyWithdrawn {
        pane: 4,
        request: 17,
    };
    assert_eq!(round_trip(&withdrawn), withdrawn);

    for answer in [
        PasskeyAnswer::Created(Created {
            id: Bytes(vec![1, 2]),
            client_data_json: Bytes(b"{}".to_vec()),
            attestation_object: Bytes(vec![0xa0]),
            transports: vec!["internal".into(), "hybrid".into()],
            attachment: Some("platform".into()),
            discoverable: Some(true),
        }),
        PasskeyAnswer::refused(PasskeyError::NotAllowed, "cancelled"),
    ] {
        let answered = AppRequest::Passkey {
            pane: 4,
            request: 17,
            answer,
        };
        assert_eq!(round_trip(&answered), answered);
        assert_eq!(answered.pane(), Some(4));
    }
}

#[test]
fn a_creation_without_optional_members_takes_their_defaults() {
    let creation: Creation = serde_json::from_value(serde_json::json!({
        "rp": {"id": "example.com"},
        "user": {"id": "AQ"},
        "challenge": "AA",
    }))
    .expect("the least a page sends");
    assert!(creation.pub_key_cred_params.is_empty());
    assert!(creation.exclude_credentials.is_empty());
    assert_eq!(
        creation.authenticator_selection.authenticator_attachment,
        None
    );
    assert!(!creation.extensions.cred_props);
}

#[test]
fn every_error_has_the_name_the_page_sees() {
    assert_eq!(PasskeyError::NotAllowed.name(), "NotAllowedError");
    assert_eq!(PasskeyError::Type.name(), "TypeError");
    assert_eq!(
        serde_json::to_value(PasskeyError::InvalidState).expect("json"),
        "invalidState"
    );
}
