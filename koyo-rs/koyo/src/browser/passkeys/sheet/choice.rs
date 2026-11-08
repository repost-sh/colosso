//! Which of the system's requests a page's ask becomes. The platform request offers the
//! person's passkeys (iCloud Keychain and their other credential providers) and, with
//! hybrid on, a phone by QR code; the security-key request offers a key over USB, NFC or
//! Bluetooth. The sheet shows the options of every request it's given.

use koyo_browser_ipc::{
    Assertion, Creation, CredentialDescriptor, PasskeyAsk, PasskeyError, Refusal,
};

/// ES256, the only algorithm platform passkeys make.
const ES256: i64 = -7;
/// RS256.
const RS256: i64 = -257;
/// The algorithms a security key is asked for, of those the page takes: the ones keys make
/// and a page's `getPublicKey()` reads (ES256, `EdDSA` and RS256).
const SECURITY_KEY_ALGORITHMS: [i64; 3] = [ES256, -8, RS256];

/// How a passkey the page names is reached, as `CredentialDescriptor::transports` says.
const PLATFORM_TRANSPORTS: [&str; 2] = ["internal", "hybrid"];
pub(super) const SECURITY_KEY_TRANSPORTS: [&str; 3] = ["usb", "nfc", "ble"];

/// The requests an ask becomes.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Choice<'a> {
    /// The platform request, if any.
    pub platform: Option<Platform<'a>>,
    /// The security-key request, if any.
    pub security_key: Option<SecurityKey<'a>>,
}

/// The platform request.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Platform<'a> {
    /// Whether the sheet offers a phone by QR code too.
    pub hybrid: bool,
    /// For a new passkey, those it mustn't make again; for a sign-in, those the site takes
    /// (any of the site's when empty).
    pub credentials: Vec<&'a CredentialDescriptor>,
}

/// The security-key request.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SecurityKey<'a> {
    /// For a new passkey, the algorithms the key may make, in the page's order.
    pub algorithms: Vec<i64>,
    /// As [`Platform::credentials`].
    pub credentials: Vec<&'a CredentialDescriptor>,
}

/// The requests `ask` becomes, when the system takes a browser's security-key requests or
/// not (`security_keys`); or why there are none.
pub(super) fn choose(ask: &PasskeyAsk, security_keys: bool) -> Result<Choice<'_>, Refusal> {
    match ask {
        PasskeyAsk::Create(creation) => creating(creation, security_keys),
        PasskeyAsk::Get(assertion) => signing_in(assertion, security_keys),
    }
}

/// A new passkey: where the page's attachment and algorithms allow it.
fn creating(creation: &Creation, security_keys: bool) -> Result<Choice<'_>, Refusal> {
    // `platform` is this Mac's passkeys alone. `cross-platform`, none, or one WebAuthn
    // doesn't know is a security key or a phone, which the platform request offers.
    let only_platform = creation
        .authenticator_selection
        .authenticator_attachment
        .as_deref()
        == Some("platform");
    // The host has filled in `WebAuthn`'s default for a page that named none.
    let algorithms: Vec<i64> = creation
        .pub_key_cred_params
        .iter()
        .map(|parameters| parameters.alg)
        .collect();
    let excluded: Vec<&CredentialDescriptor> = creation.exclude_credentials.iter().collect();
    let platform = algorithms.contains(&ES256).then(|| Platform {
        hybrid: !only_platform,
        credentials: excluded.clone(),
    });
    let key_algorithms: Vec<i64> = algorithms
        .iter()
        .copied()
        .filter(|algorithm| SECURITY_KEY_ALGORITHMS.contains(algorithm))
        .collect();
    let security_key =
        (!only_platform && security_keys && !key_algorithms.is_empty()).then_some(SecurityKey {
            algorithms: key_algorithms,
            credentials: excluded,
        });
    if platform.is_none() && security_key.is_none() {
        return Err(Refusal::new(
            PasskeyError::NotSupported,
            "No authenticator here makes the key types the site takes.",
        ));
    }
    Ok(Choice {
        platform,
        security_key,
    })
}

/// A sign-in: both requests, each with the passkeys the site names that it could reach; a
/// request with none of a list the site gave is left out, as it would offer any.
fn signing_in(assertion: &Assertion, security_keys: bool) -> Result<Choice<'_>, Refusal> {
    let allowed = &assertion.allow_credentials;
    let platform: Vec<&CredentialDescriptor> = allowed
        .iter()
        .filter(|descriptor| reached_by(descriptor, &PLATFORM_TRANSPORTS))
        .collect();
    let keys: Vec<&CredentialDescriptor> = allowed
        .iter()
        .filter(|descriptor| reached_by(descriptor, &SECURITY_KEY_TRANSPORTS))
        .collect();
    let platform = (allowed.is_empty() || !platform.is_empty()).then_some(Platform {
        hybrid: true,
        credentials: platform,
    });
    let security_key =
        (security_keys && (allowed.is_empty() || !keys.is_empty())).then_some(SecurityKey {
            algorithms: Vec::new(),
            credentials: keys,
        });
    if platform.is_none() && security_key.is_none() {
        return Err(Refusal::new(
            PasskeyError::NotAllowed,
            "None of the site's passkeys can be reached here.",
        ));
    }
    Ok(Choice {
        platform,
        security_key,
    })
}

/// Whether `descriptor` may be reached by one of `transports`: it names one of them, or none
/// Koyo knows (which says nothing of how it's reached).
fn reached_by(descriptor: &CredentialDescriptor, transports: &[&str]) -> bool {
    let named = |known: &[&str]| {
        descriptor
            .transports
            .iter()
            .any(|transport| known.contains(&transport.as_str()))
    };
    named(transports) || !(named(&PLATFORM_TRANSPORTS) || named(&SECURITY_KEY_TRANSPORTS))
}
