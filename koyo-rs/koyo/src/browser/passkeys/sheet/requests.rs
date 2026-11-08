//! The system's requests for a page's ask: a browser's own (`…WithClientData:`), whose client
//! data names the page's origin rather than the app, so the system signs for any site.

use koyo_browser_ipc::{Assertion, Bytes, Creation, CredentialDescriptor, PasskeyAsk};
use objc2::AllocAnyThread as _;
use objc2::rc::Retained;
use objc2_authentication_services::{
    ASAuthorizationAllSupportedPublicKeyCredentialDescriptorTransports,
    ASAuthorizationPlatformPublicKeyCredentialDescriptor,
    ASAuthorizationPlatformPublicKeyCredentialProvider,
    ASAuthorizationPublicKeyCredentialAssertionRequest as _,
    ASAuthorizationPublicKeyCredentialAttestationKind,
    ASAuthorizationPublicKeyCredentialAttestationKindDirect,
    ASAuthorizationPublicKeyCredentialAttestationKindEnterprise,
    ASAuthorizationPublicKeyCredentialAttestationKindIndirect,
    ASAuthorizationPublicKeyCredentialAttestationKindNone,
    ASAuthorizationPublicKeyCredentialParameters,
    ASAuthorizationPublicKeyCredentialRegistrationRequest as _,
    ASAuthorizationPublicKeyCredentialResidentKeyPreference,
    ASAuthorizationPublicKeyCredentialResidentKeyPreferenceDiscouraged,
    ASAuthorizationPublicKeyCredentialResidentKeyPreferencePreferred,
    ASAuthorizationPublicKeyCredentialResidentKeyPreferenceRequired,
    ASAuthorizationPublicKeyCredentialUserVerificationPreference,
    ASAuthorizationPublicKeyCredentialUserVerificationPreferenceDiscouraged,
    ASAuthorizationPublicKeyCredentialUserVerificationPreferencePreferred,
    ASAuthorizationPublicKeyCredentialUserVerificationPreferenceRequired, ASAuthorizationRequest,
    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptor,
    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransport,
    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportBluetooth,
    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportNFC,
    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportUSB,
    ASAuthorizationSecurityKeyPublicKeyCredentialProvider,
    ASAuthorizationWebBrowserPlatformPublicKeyCredentialAssertionRequest as _,
    ASAuthorizationWebBrowserPlatformPublicKeyCredentialProvider as _,
    ASAuthorizationWebBrowserPlatformPublicKeyCredentialRegistrationRequest as _,
    ASAuthorizationWebBrowserSecurityKeyPublicKeyCredentialProvider as _,
    ASPublicKeyCredentialClientData,
};
use objc2_foundation::{NSArray, NSData, NSString};

use super::choice::{Choice, Platform, SecurityKey};

/// The requests for `ask` from a frame of `origin`, as `choice` has them. Only on macOS 13.5
/// or later, and with a security-key request on 14.4 or later, as [`super::choice::choose`]
/// was told.
pub(super) fn requests(
    origin: &str,
    ask: &PasskeyAsk,
    choice: &Choice<'_>,
) -> Vec<Retained<ASAuthorizationRequest>> {
    let mut requests = Vec::new();
    match ask {
        PasskeyAsk::Create(creation) => {
            if let Some(platform) = &choice.platform {
                requests.push(platform_creation(origin, creation, platform));
            }
            if let Some(key) = &choice.security_key {
                requests.push(key_creation(origin, creation, key));
            }
        }
        PasskeyAsk::Get(assertion) => {
            if let Some(platform) = &choice.platform {
                requests.push(platform_sign_in(origin, assertion, platform));
            }
            if let Some(key) = &choice.security_key {
                requests.push(key_sign_in(origin, assertion, key));
            }
        }
    }
    requests
}

/// A new passkey on this Mac, in the person's providers, or on a phone.
fn platform_creation(
    origin: &str,
    creation: &Creation,
    platform: &Platform<'_>,
) -> Retained<ASAuthorizationRequest> {
    let user = &creation.user;
    let selection = &creation.authenticator_selection;
    // SAFETY: macOS 13.5 or later, which has the browser's platform requests and every
    // property set here; each value is copied.
    unsafe {
        let provider =
            ASAuthorizationPlatformPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationPlatformPublicKeyCredentialProvider::alloc(),
                &NSString::from_str(&creation.rp.id),
            );
        let request = provider.createCredentialRegistrationRequestWithClientData_name_userID(
            &client_data(origin, &creation.challenge),
            &NSString::from_str(&user.name),
            &NSData::with_bytes(&user.id.0),
        );
        request.setShouldShowHybridTransport(platform.hybrid);
        if !user.display_name.is_empty() {
            request.setDisplayName(Some(&NSString::from_str(&user.display_name)));
        }
        if let Some(preference) = user_verification(selection.user_verification.as_deref()) {
            request.setUserVerificationPreference(preference);
        }
        if let Some(kind) = attestation(creation.attestation.as_deref()) {
            request.setAttestationPreference(kind);
        }
        if !platform.credentials.is_empty() {
            request.setExcludedCredentials(Some(&platform_descriptors(&platform.credentials)));
        }
        Retained::into_super(request)
    }
}

/// A new passkey on a security key.
fn key_creation(
    origin: &str,
    creation: &Creation,
    key: &SecurityKey<'_>,
) -> Retained<ASAuthorizationRequest> {
    let user = &creation.user;
    let selection = &creation.authenticator_selection;
    // A key shows the account by its display name, which a site may leave out.
    let display_name = if user.display_name.is_empty() {
        &user.name
    } else {
        &user.display_name
    };
    let parameters: Vec<_> = key
        .algorithms
        .iter()
        .filter_map(|&algorithm| isize::try_from(algorithm).ok())
        // SAFETY: a COSE algorithm, which the class takes any of.
        .map(|algorithm| unsafe {
            ASAuthorizationPublicKeyCredentialParameters::initWithAlgorithm(
                ASAuthorizationPublicKeyCredentialParameters::alloc(),
                algorithm,
            )
        })
        .collect();
    // SAFETY: macOS 14.4 or later, which has the browser's security-key requests; every
    // property set here is older. Each value is copied.
    unsafe {
        let provider =
            ASAuthorizationSecurityKeyPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationSecurityKeyPublicKeyCredentialProvider::alloc(),
                &NSString::from_str(&creation.rp.id),
            );
        let request = provider
            .createCredentialRegistrationRequestWithClientData_displayName_name_userID(
                &client_data(origin, &creation.challenge),
                &NSString::from_str(display_name),
                &NSString::from_str(&user.name),
                &NSData::with_bytes(&user.id.0),
            );
        request.setCredentialParameters(&NSArray::from_retained_slice(&parameters));
        if let Some(preference) = resident_key(selection.resident_key.as_deref()) {
            request.setResidentKeyPreference(preference);
        }
        if let Some(preference) = user_verification(selection.user_verification.as_deref()) {
            request.setUserVerificationPreference(preference);
        }
        if let Some(kind) = attestation(creation.attestation.as_deref()) {
            request.setAttestationPreference(kind);
        }
        request.setExcludedCredentials(&key_descriptors(&key.credentials));
        Retained::into_super(request)
    }
}

/// A sign-in with a passkey on this Mac, in the person's providers, or on a phone.
fn platform_sign_in(
    origin: &str,
    assertion: &Assertion,
    platform: &Platform<'_>,
) -> Retained<ASAuthorizationRequest> {
    // SAFETY: macOS 13.5 or later, which has the browser's platform requests and every
    // property set here; each value is copied.
    unsafe {
        let provider =
            ASAuthorizationPlatformPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationPlatformPublicKeyCredentialProvider::alloc(),
                &NSString::from_str(&assertion.rp_id),
            );
        let request = provider.createCredentialAssertionRequestWithClientData(&client_data(
            origin,
            &assertion.challenge,
        ));
        request.setShouldShowHybridTransport(platform.hybrid);
        if let Some(preference) = user_verification(assertion.user_verification.as_deref()) {
            request.setUserVerificationPreference(preference);
        }
        request.setAllowedCredentials(&platform_descriptors(&platform.credentials));
        Retained::into_super(request)
    }
}

/// A sign-in with a security key.
fn key_sign_in(
    origin: &str,
    assertion: &Assertion,
    key: &SecurityKey<'_>,
) -> Retained<ASAuthorizationRequest> {
    // SAFETY: macOS 14.4 or later, which has the browser's security-key requests; every
    // property set here is older. Each value is copied.
    unsafe {
        let provider =
            ASAuthorizationSecurityKeyPublicKeyCredentialProvider::initWithRelyingPartyIdentifier(
                ASAuthorizationSecurityKeyPublicKeyCredentialProvider::alloc(),
                &NSString::from_str(&assertion.rp_id),
            );
        let request = provider.createCredentialAssertionRequestWithClientData(&client_data(
            origin,
            &assertion.challenge,
        ));
        if let Some(preference) = user_verification(assertion.user_verification.as_deref()) {
            request.setUserVerificationPreference(preference);
        }
        request.setAllowedCredentials(&key_descriptors(&key.credentials));
        Retained::into_super(request)
    }
}

/// The client data the system signs: the page's challenge from `origin`, which is never
/// cross-origin (the host turns cross-origin frames down before they get here).
fn client_data(origin: &str, challenge: &Bytes) -> Retained<ASPublicKeyCredentialClientData> {
    // SAFETY: macOS 13.5 or later, which has the class; both values are copied.
    unsafe {
        ASPublicKeyCredentialClientData::initWithChallenge_origin(
            ASPublicKeyCredentialClientData::alloc(),
            &NSData::with_bytes(&challenge.0),
            &NSString::from_str(origin),
        )
    }
}

/// The passkeys `credentials` names, for the platform request.
fn platform_descriptors(
    credentials: &[&CredentialDescriptor],
) -> Retained<NSArray<ASAuthorizationPlatformPublicKeyCredentialDescriptor>> {
    let descriptors: Vec<_> = credentials
        .iter()
        // SAFETY: an id, copied.
        .map(|credential| unsafe {
            ASAuthorizationPlatformPublicKeyCredentialDescriptor::initWithCredentialID(
                ASAuthorizationPlatformPublicKeyCredentialDescriptor::alloc(),
                &NSData::with_bytes(&credential.id.0),
            )
        })
        .collect();
    NSArray::from_retained_slice(&descriptors)
}

/// The passkeys `credentials` names, for the security-key request: each over the transports
/// the site says it's reached by, or over any when it names none a key has.
fn key_descriptors(
    credentials: &[&CredentialDescriptor],
) -> Retained<NSArray<ASAuthorizationSecurityKeyPublicKeyCredentialDescriptor>> {
    let descriptors: Vec<_> = credentials
        .iter()
        .map(|credential| {
            let named: Vec<&NSString> = credential
                .transports
                .iter()
                .filter_map(|name| transport(name))
                .collect();
            let transports = if named.is_empty() {
                // SAFETY: a function of the framework's, with no arguments.
                unsafe { ASAuthorizationAllSupportedPublicKeyCredentialDescriptorTransports() }
            } else {
                NSArray::from_slice(&named)
            };
            // SAFETY: an id and transports, both copied.
            unsafe {
                ASAuthorizationSecurityKeyPublicKeyCredentialDescriptor::initWithCredentialID_transports(
                    ASAuthorizationSecurityKeyPublicKeyCredentialDescriptor::alloc(),
                    &NSData::with_bytes(&credential.id.0),
                    &transports,
                )
            }
        })
        .collect();
    NSArray::from_retained_slice(&descriptors)
}

/// The system's name for the security-key transport `WebAuthn` calls `name`.
pub(super) fn transport(
    name: &str,
) -> Option<&'static ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransport> {
    // SAFETY: the framework's constants, set as it loads and never changed.
    unsafe {
        match name {
            "usb" => Some(ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportUSB),
            "nfc" => Some(ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportNFC),
            "ble" => {
                Some(ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransportBluetooth)
            }
            _ => None,
        }
    }
}

/// `WebAuthn`'s name for the system's security-key transport `system`; one it doesn't know, as
/// the system names it.
pub(super) fn transport_name(
    system: &ASAuthorizationSecurityKeyPublicKeyCredentialDescriptorTransport,
) -> String {
    super::choice::SECURITY_KEY_TRANSPORTS
        .into_iter()
        .find(|name| transport(name).is_some_and(|known| known == system))
        .map_or_else(|| system.to_string(), str::to_owned)
}

/// The system's `userVerification` preference for `WebAuthn`'s `value`; none leaves its default,
/// `preferred`, as `WebAuthn`'s is.
fn user_verification(
    value: Option<&str>,
) -> Option<&'static ASAuthorizationPublicKeyCredentialUserVerificationPreference> {
    // SAFETY: the framework's constants, set as it loads and never changed.
    unsafe {
        match value? {
            "required" => ASAuthorizationPublicKeyCredentialUserVerificationPreferenceRequired,
            "preferred" => ASAuthorizationPublicKeyCredentialUserVerificationPreferencePreferred,
            "discouraged" => {
                ASAuthorizationPublicKeyCredentialUserVerificationPreferenceDiscouraged
            }
            _ => None,
        }
    }
}

/// The system's `residentKey` preference for `WebAuthn`'s `value`, which is `discouraged` when
/// the page gave none (the host has made `requireResidentKey` `required` already).
fn resident_key(
    value: Option<&str>,
) -> Option<&'static ASAuthorizationPublicKeyCredentialResidentKeyPreference> {
    // SAFETY: the framework's constants, set as it loads and never changed.
    unsafe {
        match value {
            Some("required") => ASAuthorizationPublicKeyCredentialResidentKeyPreferenceRequired,
            Some("preferred") => ASAuthorizationPublicKeyCredentialResidentKeyPreferencePreferred,
            _ => ASAuthorizationPublicKeyCredentialResidentKeyPreferenceDiscouraged,
        }
    }
}

/// The system's attestation kind for `WebAuthn`'s `value`; none leaves its default, `none`, as
/// `WebAuthn`'s is.
fn attestation(
    value: Option<&str>,
) -> Option<&'static ASAuthorizationPublicKeyCredentialAttestationKind> {
    // SAFETY: the framework's constants, set as it loads and never changed.
    unsafe {
        match value? {
            "none" => ASAuthorizationPublicKeyCredentialAttestationKindNone,
            "indirect" => ASAuthorizationPublicKeyCredentialAttestationKindIndirect,
            "direct" => ASAuthorizationPublicKeyCredentialAttestationKindDirect,
            "enterprise" => ASAuthorizationPublicKeyCredentialAttestationKindEnterprise,
            _ => None,
        }
    }
}
