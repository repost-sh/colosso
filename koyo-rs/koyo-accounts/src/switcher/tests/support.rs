//! What the usage half's tests share. cswap's tests patch `oauth.try_refresh_oauth_credentials`
//! and `oauth.try_fetch_usage_for_account`; here the token endpoint and the usage endpoint are
//! the fake HTTP's answers (a closure's, when the world must change while a request is in
//! flight), and a Keychain write cswap's tests make raise is one [`Faulty`] refuses.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Map, Value, json};

use super::{Harness, sample_sequence_data};
use crate::Clock as _;
use crate::credentials::{ActiveVerdict, StashMeta, StashReason, backup_username};
use crate::http::{Request, Response};
use crate::keychain::{KeychainError, Output, Security};
use crate::oauth::{OAUTH_TOKEN_URL, USAGE_URL};
use crate::paths::BACKUP_KEYCHAIN_SERVICE;
use crate::switcher::info::AccountInfo;
use crate::switcher::{RosterSlot, SlotIdentity};
use crate::testing::{FakeSecurity, json_response, record_as};
use crate::usage_store::{FetchRecord, Identities};

/// The email of slot 1 in the gate's tests (`sample_sequence_data` with it set).
pub(super) const EMAIL: &str = "test@example.com";

/// A credential with these tokens and `expiresAt`.
pub(super) fn creds(access: &str, refresh: &str, expires_at: Value) -> String {
    let mut oauth = Map::new();
    oauth.insert("accessToken".into(), access.into());
    oauth.insert("refreshToken".into(), refresh.into());
    oauth.insert("expiresAt".into(), expires_at);
    json!({"claudeAiOauth": oauth}).to_string()
}

/// A credential with these tokens whose access token expires in the far future (cswap's
/// `9_999_999_999_000`).
pub(super) fn fresh(access: &str, refresh: &str) -> String {
    creds(access, refresh, json!(9_999_999_999_000_u64))
}

/// cswap's `_OLD`: an expired generation.
pub(super) fn old() -> String {
    creds("sk-old", "rt-old", json!(1000))
}

/// cswap's `_NEW`: a fresh one.
pub(super) fn new() -> String {
    fresh("sk-new", "rt-new")
}

/// A successor parked as the gate parks one (the production row), for `reason`, after slot
/// 1's grant of the generation `consumed_fp` was spent: its entry id.
pub(super) fn stash_successor(
    h: &Harness,
    successor: &str,
    reason: StashReason,
    consumed_fp: Option<&str>,
) -> String {
    let meta = StashMeta::successor(reason, "1", consumed_fp, successor);
    h.switcher.store.stash_successor(successor, &meta).unwrap()
}

/// One row of accounts info for slot `num` (uuid-less, organization name `Org`, no backup
/// counted), active with a clean verdict when `is_active`.
pub(super) fn info(num: &str, email: &str, org: &str, is_active: bool, creds: &str) -> AccountInfo {
    AccountInfo {
        slot: RosterSlot {
            number: num.into(),
            identity: SlotIdentity {
                email: email.into(),
                organization_uuid: org.into(),
                uuid: String::new(),
            },
            organization_name: "Org".into(),
            has_backup: false,
            disabled: false,
        },
        active: is_active.then(ActiveVerdict::default),
        creds: creds.into(),
    }
}

/// A success holding `usage`, recorded unfenced for `num` of `identities` as it was `age_s`
/// ago.
pub(super) fn recorded(h: &Harness, num: &str, usage: Value, identities: &Identities, age_s: f64) {
    let now = h.sandbox.clock.now();
    h.sandbox.clock.set(now - age_s);
    record_as(
        &h.switcher.usage_store,
        num,
        FetchRecord::success(Some(usage)),
        identities,
    );
    h.sandbox.clock.set(now);
}

/// A credential's `(accessToken, refreshToken)`.
pub(super) fn tokens(creds: &str) -> (String, String) {
    let value: Value = serde_json::from_str(creds).unwrap();
    let field = |key: &str| {
        value["claudeAiOauth"][key]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    (field("accessToken"), field("refreshToken"))
}

/// cswap's `TestConsumeGate._switcher`: `sample_sequence_data` with slot 1's email set.
pub(super) fn gate_harness() -> Harness {
    let h = Harness::new();
    let mut data = sample_sequence_data();
    data["accounts"]["1"]["email"] = EMAIL.into();
    h.write_sequence(&data);
    h
}

/// The token endpoint's answer to a refresh grant.
pub(super) fn grant(access: &str, refresh: &str) -> Response {
    json_response(
        200,
        &json!({"access_token": access, "refresh_token": refresh, "expires_in": 3600}),
    )
}

/// The next refresh POST is granted `(access, refresh)`.
pub(super) fn queue_grant(h: &Harness, access: &str, refresh: &str) {
    h.sandbox
        .http
        .queue(OAUTH_TOKEN_URL, Ok(grant(access, refresh)));
}

/// The next refresh POST answers `invalid_grant`.
pub(super) fn queue_invalid_grant(h: &Harness) {
    h.sandbox
        .http
        .queue_json(OAUTH_TOKEN_URL, 400, &json!({"error": "invalid_grant"}));
}

/// The refresh token a request sent.
pub(super) fn sent_refresh_token(request: &Request) -> String {
    let body: Value = serde_json::from_str(request.body.as_deref().unwrap_or("{}")).unwrap();
    body["refresh_token"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// The refresh tokens sent to the token endpoint, in order.
pub(super) fn posted(h: &Harness) -> Vec<String> {
    h.sandbox
        .http
        .requests_to(OAUTH_TOKEN_URL)
        .iter()
        .map(sent_refresh_token)
        .collect()
}

/// The next usage request answers `body` (the endpoint's raw shape).
pub(super) fn queue_usage(h: &Harness, body: &Value) {
    h.sandbox.http.queue_json(USAGE_URL, 200, body);
}

/// The usage requests made.
pub(super) fn usage_requests(h: &Harness) -> usize {
    h.sandbox.http.requests_to(USAGE_URL).len()
}

/// The access tokens the requests to URLs starting with `prefix` carried, in order.
pub(super) fn bearers(h: &Harness, prefix: &str) -> Vec<String> {
    h.sandbox
        .http
        .requests_to(prefix)
        .iter()
        .filter_map(|request| {
            request
                .headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.trim_start_matches("Bearer ").to_owned())
        })
        .collect()
}

/// The text of a `security` call: its stdin line, or its arguments.
pub(super) fn call_text(call: &crate::testing::Call) -> String {
    call.stdin.clone().unwrap_or_else(|| call.args.join(" "))
}

pub(super) fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// `security` that refuses (exit 1) the calls its rule picks, given the call's command line,
/// and passes the rest to the sandbox's fake.
pub(super) struct Faulty {
    pub inner: Arc<FakeSecurity>,
    pub rule: Box<dyn Fn(&str) -> bool + Send + Sync>,
}

impl Security for Faulty {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError> {
        let line = input.map_or_else(|| args.join(" "), str::to_owned);
        if (self.rule)(&line) {
            return Ok(Output {
                status: 1,
                ..Output::default()
            });
        }
        self.inner.run(args, input)
    }
}

/// A [`Faulty`] that, once `armed`, refuses every write of slot `num`'s backup item (its
/// `.prev` keeps working), as cswap's tests make `_write_account_credentials` raise.
pub(super) fn backup_write_refuser(
    h: &Harness,
    num: &str,
    email: &str,
) -> (Arc<dyn Security>, Arc<AtomicBool>) {
    let armed = Arc::new(AtomicBool::new(false));
    let account = format!("\"{}\"", backup_username(num, email));
    let service = format!("\"{BACKUP_KEYCHAIN_SERVICE}\"");
    let trigger = armed.clone();
    let security = Arc::new(Faulty {
        inner: h.sandbox.security.clone(),
        rule: Box::new(move |line| {
            trigger.load(Ordering::SeqCst)
                && line.starts_with("add-generic-password")
                && line.contains(&service)
                && line.contains(&account)
        }),
    });
    (security, armed)
}
