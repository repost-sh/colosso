//! What the sheet asks for and answers, short of showing it: the requests an ask becomes, the
//! system's objects for them, and how its endings read to the page.

use koyo_browser_ipc::{
    Assertion, AuthenticatorSelection, Bytes, Creation, CredentialDescriptor, CredentialParameters,
    Extensions, PasskeyAnswer, PasskeyAsk, PasskeyError, RelyingParty, User,
};
use objc2::available;
use objc2::rc::Retained;
use objc2_authentication_services::{
    ASAuthorizationPlatformPublicKeyCredentialAssertionRequest,
    ASAuthorizationPlatformPublicKeyCredentialRegistrationRequest,
    ASAuthorizationPublicKeyCredentialDescriptor as _,
    ASAuthorizationPublicKeyCredentialRegistrationRequest as _,
    ASAuthorizationPublicKeyCredentialResidentKeyPreferenceRequired, ASAuthorizationRequest,
    ASAuthorizationSecurityKeyPublicKeyCredentialAssertionRequest,
    ASAuthorizationSecurityKeyPublicKeyCredentialRegistrationRequest,
    ASAuthorizationWebBrowserPlatformPublicKeyCredentialAssertionRequest as _,
    ASAuthorizationWebBrowserPlatformPublicKeyCredentialRegistrationRequest as _,
};

use super::answer::{self, Asked, Authenticator, Registration};
use super::choice::{Choice, Platform, SecurityKey, choose};
use super::requests::{requests, transport, transport_name};

const ORIGIN: &str = "https://accounts.example.com";

/// A passkey of the site's, by an id of 16 `id`s.
fn descriptor(id: u8, transports: &[&str]) -> CredentialDescriptor {
    CredentialDescriptor {
        id: Bytes(vec![id; 16]),
        transports: transports.iter().map(|&name| name.to_owned()).collect(),
    }
}

/// A new passkey's options, as a site asks: of `attachment`, for `algorithms`, discoverable,
/// with the account's passkey on this Mac excluded.
fn creation(attachment: Option<&str>, algorithms: &[i64]) -> Creation {
    Creation {
        rp: RelyingParty {
            id: "example.com".to_owned(),
        },
        user: User {
            id: Bytes(vec![7; 8]),
            name: "ada@example.com".to_owned(),
            display_name: "Ada".to_owned(),
        },
        challenge: Bytes(vec![1; 32]),
        pub_key_cred_params: algorithms
            .iter()
            .map(|&alg| CredentialParameters { alg })
            .collect(),
        exclude_credentials: vec![descriptor(9, &["internal"])],
        authenticator_selection: AuthenticatorSelection {
            authenticator_attachment: attachment.map(str::to_owned),
            resident_key: Some("required".to_owned()),
            user_verification: Some("required".to_owned()),
        },
        attestation: Some("none".to_owned()),
        extensions: Extensions { cred_props: true },
    }
}

/// A sign-in's options, with `allowed` the site's passkeys.
fn assertion(allowed: Vec<CredentialDescriptor>) -> Assertion {
    Assertion {
        challenge: Bytes(vec![2; 32]),
        rp_id: "example.com".to_owned(),
        allow_credentials: allowed,
        user_verification: Some("preferred".to_owned()),
    }
}

/// Which requests `choice` holds: the platform's with whether it offers a phone, and the
/// security key's with its algorithms.
fn kinds(choice: &Choice<'_>) -> (Option<bool>, Option<Vec<i64>>) {
    (
        choice.platform.as_ref().map(|platform| platform.hybrid),
        choice
            .security_key
            .as_ref()
            .map(|key| key.algorithms.clone()),
    )
}

#[test]
fn a_new_passkey_is_asked_of_the_authenticators_its_attachment_and_algorithms_allow() {
    let made = |attachment, algorithms: &[i64], keys| {
        let ask = PasskeyAsk::Create(creation(attachment, algorithms));
        choose(&ask, keys).map(|choice| kinds(&choice))
    };
    let both = [-7, -257];
    // This Mac's passkeys alone, with no phone.
    assert_eq!(made(Some("platform"), &both, true), Ok((Some(false), None)));
    // A security key, or a phone through the platform request.
    let keys = Some(vec![-7, -257]);
    assert_eq!(
        made(Some("cross-platform"), &both, true),
        Ok((Some(true), keys.clone()))
    );
    assert_eq!(made(None, &both, true), Ok((Some(true), keys.clone())));
    assert_eq!(made(Some("elsewhere"), &both, true), Ok((Some(true), keys)));
    // Platform passkeys are ES256 only; keys are asked for what they make, in the page's order.
    assert_eq!(
        made(None, &[-257, -8], true),
        Ok((None, Some(vec![-257, -8])))
    );
    assert_eq!(
        made(None, &[-36, -7], true),
        Ok((Some(true), Some(vec![-7])))
    );
    // A system without the browser's security-key requests offers the platform's alone.
    assert_eq!(made(None, &both, false), Ok((Some(true), None)));
}

#[test]
fn a_new_passkey_no_authenticator_here_can_make_is_not_supported() {
    let refused = |attachment, algorithms: &[i64], keys| {
        let ask = PasskeyAsk::Create(creation(attachment, algorithms));
        choose(&ask, keys)
            .map(|_| ())
            .map_err(|refusal| refusal.error)
    };
    assert_eq!(
        refused(Some("platform"), &[-257], true),
        Err(PasskeyError::NotSupported)
    );
    assert_eq!(
        refused(Some("cross-platform"), &[-257], false),
        Err(PasskeyError::NotSupported)
    );
    assert_eq!(refused(None, &[-36], true), Err(PasskeyError::NotSupported));
}

#[test]
fn a_new_passkey_mustnt_be_any_the_account_has_on_either_authenticator() {
    let creation = creation(None, &[-7]);
    let ask = PasskeyAsk::Create(creation.clone());
    let choice = choose(&ask, true).unwrap();
    let excluded = vec![&creation.exclude_credentials[0]];
    assert_eq!(
        choice,
        Choice {
            platform: Some(Platform {
                hybrid: true,
                credentials: excluded.clone(),
            }),
            security_key: Some(SecurityKey {
                algorithms: vec![-7],
                credentials: excluded,
            }),
        }
    );
}

#[test]
fn a_sign_in_offers_each_authenticator_the_passkeys_it_could_reach() {
    let ask = PasskeyAsk::Get(assertion(vec![
        descriptor(1, &["internal"]),
        descriptor(2, &["usb", "nfc"]),
        // No transports, both kinds, and one Koyo doesn't know: any of them may reach it.
        descriptor(3, &[]),
        descriptor(4, &["hybrid", "ble"]),
        descriptor(5, &["smart-card"]),
    ]));
    let choice = choose(&ask, true).unwrap();
    let ids = |credentials: &[&CredentialDescriptor]| -> Vec<u8> {
        credentials
            .iter()
            .map(|credential| credential.id.0[0])
            .collect()
    };
    let platform = choice.platform.unwrap();
    assert!(platform.hybrid);
    assert_eq!(ids(&platform.credentials), [1, 3, 4, 5]);
    assert_eq!(ids(&choice.security_key.unwrap().credentials), [2, 3, 4, 5]);
}

#[test]
fn a_sign_in_leaves_out_an_authenticator_with_none_of_the_sites_passkeys() {
    let kinds_for = |allowed: Vec<CredentialDescriptor>, keys| {
        choose(&PasskeyAsk::Get(assertion(allowed)), keys).map(|choice| kinds(&choice))
    };
    // Any of the site's passkeys, on either.
    assert_eq!(
        kinds_for(Vec::new(), true),
        Ok((Some(true), Some(Vec::new())))
    );
    assert_eq!(kinds_for(Vec::new(), false), Ok((Some(true), None)));
    // Passkeys on this Mac only, then on keys only.
    assert_eq!(
        kinds_for(vec![descriptor(1, &["internal"])], true),
        Ok((Some(true), None))
    );
    assert_eq!(
        kinds_for(vec![descriptor(2, &["usb"])], true),
        Ok((None, Some(Vec::new())))
    );
    // Keys only, where the system doesn't take a browser's security-key requests.
    let ask = PasskeyAsk::Get(assertion(vec![descriptor(2, &["usb"])]));
    let refused = choose(&ask, false);
    assert_eq!(
        refused.map(|_| ()).map_err(|refusal| refusal.error),
        Err(PasskeyError::NotAllowed)
    );
}

#[test]
fn the_sheets_errors_read_as_the_domexceptions_sites_expect() {
    let error = |ours, code| answer::refusal(ours, code).error;
    for code in [1000, 1001, 1002, 1003, 1004, 1005, 1007, 1010, 0, -1] {
        assert_eq!(error(true, code), PasskeyError::NotAllowed, "{code}");
    }
    assert_eq!(error(true, 1006), PasskeyError::InvalidState);
    // Another domain's 1006 is no excluded passkey.
    assert_eq!(error(false, 1006), PasskeyError::NotAllowed);
    assert_eq!(
        answer::refusal(true, 1001).message,
        "The request was cancelled."
    );
}

/// A platform passkey the system made, with `attestation_object`.
fn registration(attestation_object: Option<Vec<u8>>) -> Registration {
    Registration {
        id: vec![9; 4],
        client_data_json: b"{}".to_vec(),
        attestation_object,
        attachment: answer::PLATFORM,
        transports: vec!["hybrid".to_owned(), "internal".to_owned()],
    }
}

#[test]
fn a_new_passkey_says_its_discoverable_only_when_asked_and_known() {
    let asked = |cred_props, resident_key_required| Asked {
        cred_props,
        resident_key_required,
    };
    let made = |by, asked| answer::created(registration(Some(vec![0xa0])), by, asked);
    let PasskeyAnswer::Created(created) = made(Authenticator::Platform, asked(true, false)) else {
        panic!("not made");
    };
    assert_eq!(created.id.0, [9; 4]);
    assert_eq!(created.attestation_object.0, [0xa0]);
    assert_eq!(created.attachment.as_deref(), Some("platform"));
    assert_eq!(created.discoverable, Some(true));
    let discoverable = |by, asked| match made(by, asked) {
        PasskeyAnswer::Created(created) => created.discoverable,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        discoverable(Authenticator::Platform, asked(false, true)),
        None
    );
    assert_eq!(
        discoverable(Authenticator::SecurityKey, asked(true, true)),
        Some(true)
    );
    assert_eq!(
        discoverable(Authenticator::SecurityKey, asked(true, false)),
        None
    );
    // No attestation object: the page can't be given the key.
    let answer = answer::created(
        registration(None),
        Authenticator::Platform,
        asked(false, false),
    );
    let PasskeyAnswer::Refused(refusal) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(refusal.error, PasskeyError::Unknown);
}

#[test]
fn a_sign_in_without_a_user_handle_says_none() {
    assert_eq!(answer::user_handle(Vec::new()), None);
    assert_eq!(answer::user_handle(vec![7]), Some(Bytes(vec![7])));
}

#[test]
fn security_key_transports_go_by_the_names_webauthn_gives_them() {
    for name in ["usb", "nfc", "ble"] {
        assert_eq!(transport_name(transport(name).unwrap()), name);
    }
    assert!(transport("internal").is_none());
}

/// The request at `at` of `requests`, as a `T`.
fn request<T: objc2::DowncastTarget>(
    requests: &[Retained<ASAuthorizationRequest>],
    at: usize,
) -> &T {
    requests[at].downcast_ref::<T>().unwrap()
}

#[test]
fn the_requests_carry_the_pages_ask_to_the_system() {
    if !available!(macos = 14.4) {
        return;
    }
    let ask = PasskeyAsk::Create(creation(None, &[-7, -257]));
    let made = requests(ORIGIN, &ask, &choose(&ask, true).unwrap());
    assert_eq!(made.len(), 2);
    let platform: &ASAuthorizationPlatformPublicKeyCredentialRegistrationRequest =
        request(&made, 0);
    let key: &ASAuthorizationSecurityKeyPublicKeyCredentialRegistrationRequest = request(&made, 1);
    // SAFETY: the requests just made, on a system that has every property read.
    unsafe {
        assert!(platform.shouldShowHybridTransport());
        assert_eq!(platform.name().to_string(), "ada@example.com");
        assert_eq!(platform.displayName().unwrap().to_string(), "Ada");
        assert_eq!(platform.userID().to_vec(), [7; 8]);
        assert_eq!(platform.relyingPartyIdentifier().to_string(), "example.com");
        assert_eq!(platform.clientData().unwrap().origin().to_string(), ORIGIN);
        assert_eq!(platform.excludedCredentials().unwrap().len(), 1);
        let algorithms: Vec<isize> = key
            .credentialParameters()
            .iter()
            .map(|parameters| parameters.algorithm())
            .collect();
        assert_eq!(algorithms, [-7, -257]);
        let required = ASAuthorizationPublicKeyCredentialResidentKeyPreferenceRequired.unwrap();
        assert_eq!(&*key.residentKeyPreference(), required);
        // Excluded on this Mac, so over any of a key's transports.
        let excluded = key.excludedCredentials();
        assert_eq!(excluded.len(), 1);
        assert!(!excluded.objectAtIndex(0).transports().is_empty());
    }
    let ask = PasskeyAsk::Get(assertion(vec![
        descriptor(1, &["internal"]),
        descriptor(2, &["usb"]),
    ]));
    let made = requests(ORIGIN, &ask, &choose(&ask, true).unwrap());
    let platform: &ASAuthorizationPlatformPublicKeyCredentialAssertionRequest = request(&made, 0);
    let key: &ASAuthorizationSecurityKeyPublicKeyCredentialAssertionRequest = request(&made, 1);
    // SAFETY: as above.
    unsafe {
        assert!(platform.shouldShowHybridTransport());
        assert_eq!(
            platform
                .allowedCredentials()
                .objectAtIndex(0)
                .credentialID()
                .to_vec(),
            [1; 16]
        );
        let allowed = key.allowedCredentials().objectAtIndex(0);
        assert_eq!(allowed.credentialID().to_vec(), [2; 16]);
        assert_eq!(
            allowed
                .transports()
                .iter()
                .map(|name| transport_name(&name))
                .collect::<Vec<_>>(),
            ["usb"]
        );
    }
}
