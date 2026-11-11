//! Port of claude-swap's `credentials.py` (0fe22a8), its credential-shape helpers: telling a
//! managed API key from an OAuth credential, and the machine-shared fields of an OAuth
//! credential, which a switch takes from the live credential rather than the target slot.
//!
//! A composed credential is serialised by `serde_json`, compact, where cswap's `json.dumps`
//! puts a space after `,` and `:`; readers parse it, and Claude Code writes it compact too.

use serde_json::{Map, Value};

/// Anthropic's OAuth token prefixes, the access/setup token and the refresh token: a bare
/// value carrying one is OAuth material, never an API key (Claude Code's own
/// `isAnthropicOAuthToken`).
const OAUTH_TOKEN_PREFIXES: [&str; 2] = ["sk-ant-oat", "sk-ant-ort"];

/// The siblings of `claudeAiOauth` that hold OAuth integrations shared by the machine and
/// rotated independently of any account, so on activation the live copy wins. Everything
/// else, known or not, stays with the target slot: a stale shared field only re-prompts,
/// while a live account-bound one carried across a switch presents one account's credential
/// under another.
pub const SHARED_CREDENTIAL_KEYS: [&str; 5] = [
    "mcpOAuth",
    "mcpOAuthClientConfig",
    "mcpXaaIdp",
    "mcpXaaIdpConfig",
    "pluginSecrets",
];

/// The account-scoped siblings Koyo knows: the login itself, and `trustedDeviceToken`,
/// enrolled per device and account at `/login`.
pub const ACCOUNT_CREDENTIAL_KEYS: [&str; 2] = ["claudeAiOauth", "trustedDeviceToken"];

/// Whether a stored active credential is a bare managed API key rather than OAuth JSON: any
/// `sk-ant-` value is a key unless it carries an OAuth prefix. A denylist of OAuth prefixes,
/// not an allowlist of key families, so a garbled setup token is never taken for a key and
/// every newer key family still is one (#465).
pub fn looks_like_api_key(credentials: Option<&str>) -> bool {
    let Some(credentials) = credentials.filter(|credentials| !credentials.is_empty()) else {
        return false;
    };
    let text = credentials.trim();
    if text.starts_with('{') {
        return false;
    }
    text.starts_with("sk-ant-")
        && !OAUTH_TOKEN_PREFIXES
            .iter()
            .any(|prefix| text.starts_with(prefix))
}

/// The credential as a JSON object, or `None` for nothing, a managed API key, or anything
/// that isn't an object.
pub fn credential_object(credentials: Option<&str>) -> Option<Map<String, Value>> {
    let credentials = credentials.filter(|credentials| !credentials.is_empty())?;
    if looks_like_api_key(Some(credentials)) {
        return None;
    }
    match serde_json::from_str(credentials) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// The machine-shared fields of an OAuth credential, [`SHARED_CREDENTIAL_KEYS`] only. `None`
/// when it isn't a JSON credential object; a map, empty included, is authoritative for every
/// shared key: one absent here is absent from the machine.
pub fn shared_credential_fields(credentials: Option<&str>) -> Option<Map<String, Value>> {
    let data = credential_object(credentials)?;
    if data.contains_key("claudeAiOauth") {
        // An unknown sibling stays with the slot, which fails safe, but silently: if Claude
        // Code grows a new shared key, that default brings back the stale restore for it.
        let mut unrecognized: Vec<&str> = data
            .keys()
            .map(String::as_str)
            .filter(|key| {
                !SHARED_CREDENTIAL_KEYS.contains(key) && !ACCOUNT_CREDENTIAL_KEYS.contains(key)
            })
            .collect();
        if !unrecognized.is_empty() {
            unrecognized.sort_unstable();
            log::debug!(
                "Live credential has sibling keys Koyo does not recognize (a newer Claude \
                 Code?), treating them as slot-owned: {unrecognized:?}"
            );
        }
    }
    Some(
        SHARED_CREDENTIAL_KEYS
            .iter()
            .filter_map(|key| {
                data.get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect(),
    )
}

/// The target login composed with the machine's shared fields: the target's copies of the
/// shared keys are dropped and `shared_fields` supplies the current ones, so a shared key the
/// machine no longer holds isn't brought back from the slot. Every other target field passes
/// through. A target that isn't a JSON object with `claudeAiOauth` (an API key, an opaque
/// legacy shape) comes back unchanged.
pub fn merge_shared_credential_fields(
    target_credentials: &str,
    shared_fields: &Map<String, Value>,
) -> String {
    let Some(target) = credential_object(Some(target_credentials))
        .filter(|target| target.contains_key("claudeAiOauth"))
    else {
        return target_credentials.to_owned();
    };
    let mut composed: Map<String, Value> = target
        .into_iter()
        .filter(|(key, _)| !SHARED_CREDENTIAL_KEYS.contains(&key.as_str()))
        .collect();
    composed.extend(
        shared_fields
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    Value::Object(composed).to_string()
}
