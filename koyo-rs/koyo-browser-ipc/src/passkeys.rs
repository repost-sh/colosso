//! Passkeys: a page's request to make one or sign in with one (`WebAuthn`'s
//! `navigator.credentials.create` and `get`), as the host checked it, and the app's
//! answer from the system's passkey sheet.
//!
//! The host fills in and checks the relying party's id against the frame's origin before
//! anything reaches the app, so the app only ever sees requests a browser would make. The
//! shapes follow `WebAuthn`'s JSON forms (`PublicKeyCredentialCreationOptionsJSON` and the
//! like): names in camel case, bytes as base64url.
use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes, carried as base64url without padding, as `WebAuthn`'s JSON forms carry them;
/// padding is taken too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

/// Base64url, written without padding and read with or without it.
const BASE64URL: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64URL.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        BASE64URL
            .decode(text)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// What a page asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ceremony", rename_all = "camelCase")]
pub enum PasskeyAsk {
    /// A new passkey (`navigator.credentials.create`).
    Create(Creation),
    /// A sign-in with one the person has (`navigator.credentials.get`).
    Get(Assertion),
}

impl PasskeyAsk {
    /// The relying party's id, as the host checked it.
    #[must_use]
    pub fn rp_id(&self) -> &str {
        match self {
            Self::Create(creation) => &creation.rp.id,
            Self::Get(assertion) => &assertion.rp_id,
        }
    }
}

/// A new passkey's options (`PublicKeyCredentialCreationOptionsJSON`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Creation {
    /// The site, its id checked by the host.
    pub rp: RelyingParty,
    /// The account the passkey is for.
    pub user: User,
    /// What the new passkey signs.
    pub challenge: Bytes,
    /// The key types the site takes, in its order of preference; the host fills in
    /// `WebAuthn`'s default (ES256, then RS256) when the page names none.
    #[serde(default)]
    pub pub_key_cred_params: Vec<CredentialParameters>,
    /// Passkeys the account has already, which mustn't be made again.
    #[serde(default)]
    pub exclude_credentials: Vec<CredentialDescriptor>,
    /// Which authenticators the site wants.
    #[serde(default)]
    pub authenticator_selection: AuthenticatorSelection,
    /// `none`, `indirect`, `direct` or `enterprise`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<String>,
    /// The extensions asked for that Koyo answers.
    #[serde(default)]
    pub extensions: Extensions,
}

/// A sign-in's options (`PublicKeyCredentialRequestOptionsJSON`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    /// What the passkey signs.
    pub challenge: Bytes,
    /// The site's id, checked by the host; empty from a page that names none, which the host
    /// makes the page's domain.
    #[serde(default)]
    pub rp_id: String,
    /// The passkeys the site takes; any of the site's when empty.
    #[serde(default)]
    pub allow_credentials: Vec<CredentialDescriptor>,
    /// `required`, `preferred` or `discouraged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_verification: Option<String>,
}

/// The site a passkey belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelyingParty {
    /// Its id: the page's domain or a registrable suffix of it; empty from a page that names
    /// none, which the host makes the page's domain.
    #[serde(default)]
    pub id: String,
}

/// The account a new passkey is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// The site's handle for the account, 1 to 64 bytes.
    pub id: Bytes,
    /// Its name, such as an email address.
    #[serde(default)]
    pub name: String,
    /// The name the person goes by.
    #[serde(default)]
    pub display_name: String,
}

/// A key type a site takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialParameters {
    /// The COSE algorithm: -7 for ES256, -8 for `EdDSA`, -257 for RS256.
    pub alg: i64,
}

/// A passkey, by its id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDescriptor {
    /// The credential's id.
    pub id: Bytes,
    /// How it's reached: `usb`, `nfc`, `ble`, `hybrid`, `internal`.
    #[serde(default)]
    pub transports: Vec<String>,
}

/// Which authenticators a site wants for a new passkey.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatorSelection {
    /// `platform` (this Mac's, or the person's synced passkeys) or `cross-platform` (a
    /// security key or a phone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticator_attachment: Option<String>,
    /// `required`, `preferred` or `discouraged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resident_key: Option<String>,
    /// `required`, `preferred` or `discouraged`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_verification: Option<String>,
}

/// The extensions a page asked for that Koyo answers; the others it leaves unanswered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extensions {
    /// Whether the page wants to know if the new passkey is discoverable (`credProps`).
    #[serde(default)]
    pub cred_props: bool,
}

/// The app's answer to a [`PasskeyAsk`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PasskeyAnswer {
    /// A new passkey was made.
    Created(Created),
    /// The person signed in with one.
    Got(Got),
    /// No passkey: the person cancelled, or it couldn't be done.
    Refused(Refusal),
}

impl PasskeyAnswer {
    /// No passkey, an `error` saying `message`.
    #[must_use]
    pub fn refused(error: PasskeyError, message: impl Into<String>) -> Self {
        Self::Refused(Refusal::new(error, message))
    }
}

/// A new passkey, as the authenticator made it: the host makes `RegistrationResponseJSON` of
/// it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    /// The credential's id.
    pub id: Bytes,
    /// The client data the authenticator signed, as the system made it.
    pub client_data_json: Bytes,
    /// The authenticator's attestation object (CBOR), which holds the authenticator data
    /// and the new key; the host reads them out of it for the page.
    pub attestation_object: Bytes,
    /// How the passkey can be reached from now on.
    #[serde(default)]
    pub transports: Vec<String>,
    /// `platform` or `cross-platform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<String>,
    /// Whether the passkey is discoverable, when the page asked (`credProps.rk`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discoverable: Option<bool>,
}

/// A sign-in (`AuthenticationResponseJSON`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Got {
    /// The credential's id.
    pub id: Bytes,
    /// The client data the authenticator signed, as the system made it.
    pub client_data_json: Bytes,
    /// The authenticator data.
    pub authenticator_data: Bytes,
    /// The signature over the authenticator data and the client data's hash.
    pub signature: Bytes,
    /// The account's handle, as the site gave it when the passkey was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_handle: Option<Bytes>,
    /// `platform` or `cross-platform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<String>,
}

/// Why there's no passkey, as the page's promise rejects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The error's kind.
    pub error: PasskeyError,
    /// What it says.
    pub message: String,
}

impl Refusal {
    /// A refusal of kind `error` saying `message`.
    #[must_use]
    pub fn new(error: PasskeyError, message: impl Into<String>) -> Self {
        Self {
            error,
            message: message.into(),
        }
    }
}

/// The errors a passkey request ends in, by the `DOMException` (or `TypeError`) the page
/// sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PasskeyError {
    /// `NotAllowedError`: cancelled, timed out, or not allowed here; what a site takes as
    /// "try another way".
    NotAllowed,
    /// `InvalidStateError`: the authenticator holds one of the excluded passkeys.
    InvalidState,
    /// `NotSupportedError`: none of the key types the site takes.
    NotSupported,
    /// `SecurityError`: the relying party's id doesn't fit the page's origin.
    Security,
    /// `UnknownError`: the system failed without saying why.
    Unknown,
    /// `TypeError`: the page's options are malformed.
    Type,
}

impl PasskeyError {
    /// The name the page's error has.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::NotAllowed => "NotAllowedError",
            Self::InvalidState => "InvalidStateError",
            Self::NotSupported => "NotSupportedError",
            Self::Security => "SecurityError",
            Self::Unknown => "UnknownError",
            Self::Type => "TypeError",
        }
    }
}

#[cfg(test)]
mod tests;
