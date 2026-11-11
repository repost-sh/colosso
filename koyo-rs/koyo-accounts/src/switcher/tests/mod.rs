//! The switcher's tests: claude-swap's (0fe22a8) `tests/test_switcher.py` classes for the
//! ported code, `tests/test_add_account_identity.py`, the capture classes of
//! `tests/test_session.py` and the disable-flag tests of `tests/test_account_policy_store.py`,
//! translated onto a sandbox. cswap's tests patch the switcher's reads and writes with
//! in-memory stores; here the live credential is the fake Keychain's `Claude
//! Code-credentials` item, each backup its `Koyo Claude accounts` item, and the profile oracle
//! the fake HTTP's answer (nothing queued is "unresolvable").
//!
//! The usage half's are beside them: the refresh gate (`grant*.rs`), the active and idle
//! fetches (`usage_active.rs`, `resync.rs`, `idle.rs`), collection passes (`collect*.rs`),
//! `accounts_snapshot` with `tests/test_backup_read_pacing.py`'s display cases
//! (`snapshot.rs`), the plans of `tests/test_subscription.py`'s `TestCollector` (`plans.rs`),
//! and Koyo's live limits (`live.rs`); `support.rs` has what they share. `engine.rs` runs the
//! auto-switch engine's tick on a switcher.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};

use crate::credentials::backup_username;
use crate::http::{Http, HttpError, Request, Response};
use crate::keychain::{Security, keychain_account_name};
use crate::oauth::PROFILE_URL;
use crate::paths::{BACKUP_KEYCHAIN_SERVICE, CLAUDE_CODE_KEYCHAIN_SERVICE};
use crate::switcher::Switcher;
use crate::testing::Sandbox;

mod add;
mod add_identity;
mod capture;
mod collect;
mod collect_passes;
mod direct;
mod engine;
mod fallback;
mod grant;
mod grant_faults;
mod grant_locks;
mod grant_stash;
mod identity;
mod idle;
mod live;
mod plans;
mod policy;
mod provenance;
mod resync;
mod snapshot;
mod store;
mod support;
mod switch;
mod switch_to;
mod usage_active;

/// A switcher on a sandbox with cswap's `temp_home`: a home with `.claude/` in it.
pub(super) struct Harness {
    pub sandbox: Sandbox,
    pub switcher: Switcher,
}

impl Harness {
    pub fn new() -> Harness {
        Harness::on(Sandbox::new())
    }

    /// On `sandbox` as its variables are now.
    pub fn on(sandbox: Sandbox) -> Harness {
        std::fs::create_dir_all(sandbox.home().join(".claude")).unwrap();
        let switcher = Switcher::new(sandbox.env());
        Harness { sandbox, switcher }
    }

    /// A fresh switcher on the sandbox, after its variables changed.
    pub fn rebuild(&mut self) {
        self.switcher = Switcher::new(self.sandbox.env());
    }

    /// A fresh switcher whose HTTP is `http` (an oracle with side effects).
    pub fn rebuild_with_http(&mut self, http: Arc<dyn Http>) {
        let security = self.sandbox.security.clone();
        self.rebuild_with(security, http);
    }

    /// A fresh switcher whose `security` is `security`.
    pub fn rebuild_with_security(&mut self, security: Arc<dyn Security>) {
        let http = self.sandbox.http.clone();
        self.rebuild_with(security, http);
    }

    /// A fresh switcher on the sandbox with this `security` and this HTTP.
    pub fn rebuild_with(&mut self, security: Arc<dyn Security>, http: Arc<dyn Http>) {
        let mut env = self.sandbox.env();
        env.security = security;
        env.http = http;
        self.switcher = Switcher::new(env);
    }

    pub fn home(&self) -> PathBuf {
        self.sandbox.home()
    }

    /// The Keychain account of Claude Code's items, as the sandbox's `$USER` gives it.
    pub fn user(&self) -> String {
        keychain_account_name(self.sandbox.vars.user.as_deref())
    }

    /// `~/.claude.json`.
    pub fn config_path(&self) -> PathBuf {
        self.home().join(".claude.json")
    }

    pub fn write_config(&self, config: &Value) {
        std::fs::write(self.config_path(), config.to_string()).unwrap();
    }

    /// `~/.claude.json`, parsed.
    pub fn config(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.config_path()).unwrap()).unwrap()
    }

    /// cswap's `mock_claude_config`.
    pub fn mock_claude_config(&self) {
        self.write_config(&json!({"oauthAccount": {
            "emailAddress": "test@example.com", "accountUuid": "test-uuid-1234",
        }}));
    }

    pub fn write_sequence(&self, data: &Value) {
        self.switcher.setup_directories().unwrap();
        self.switcher
            .write_json(&self.switcher.sequence_file, data)
            .unwrap();
    }

    pub fn sequence(&self) -> Value {
        Value::Object(
            self.switcher
                .get_sequence_data()
                .unwrap()
                .unwrap_or_default(),
        )
    }

    pub fn accounts(&self) -> Map<String, Value> {
        self.sequence()["accounts"]
            .as_object()
            .cloned()
            .unwrap_or_default()
    }

    /// The live credential, Claude Code's Keychain item.
    pub fn set_live(&self, creds: &str) {
        self.sandbox
            .security
            .set(CLAUDE_CODE_KEYCHAIN_SERVICE, &self.user(), creds);
    }

    pub fn live(&self) -> Option<String> {
        self.sandbox
            .security
            .get(CLAUDE_CODE_KEYCHAIN_SERVICE, &self.user())
    }

    /// The live credential's access token.
    pub fn live_access_token(&self) -> String {
        let live: Value = serde_json::from_str(&self.live().unwrap()).unwrap();
        live["claudeAiOauth"]["accessToken"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    /// `~/.claude/.credentials.json`.
    pub fn credentials_file(&self) -> PathBuf {
        self.home().join(".claude/.credentials.json")
    }

    pub fn set_backup(&self, num: &str, email: &str, creds: &str) {
        self.sandbox
            .security
            .set(BACKUP_KEYCHAIN_SERVICE, &backup_username(num, email), creds);
    }

    pub fn backup(&self, num: &str, email: &str) -> Option<String> {
        self.sandbox
            .security
            .get(BACKUP_KEYCHAIN_SERVICE, &backup_username(num, email))
    }

    /// Every backup item's account.
    pub fn backup_accounts(&self) -> Vec<String> {
        self.sandbox
            .security
            .items()
            .into_keys()
            .filter(|(service, _)| service == BACKUP_KEYCHAIN_SERVICE)
            .map(|(_, account)| account)
            .collect()
    }

    pub fn set_config_backup(&self, num: &str, email: &str, config: &Value) {
        self.switcher.setup_directories().unwrap();
        self.switcher
            .write_account_config(num, email, &config.to_string())
            .unwrap();
    }

    pub fn config_backup(&self, num: &str, email: &str) -> String {
        self.switcher.read_account_config(num, email).unwrap()
    }

    /// The next profile lookups answer `body` (cswap's `fetch_oauth_profile` patched).
    pub fn queue_profile(&self, body: &Value) {
        for _ in 0..4 {
            self.sandbox.http.queue_json(PROFILE_URL, 200, body);
        }
    }

    pub fn profile_lookups(&self) -> usize {
        self.sandbox.http.requests_to(PROFILE_URL).len()
    }

    /// The safety copies' rows.
    pub fn stash(&self) -> Map<String, Value> {
        self.switcher.store.list_unclaimed_credentials()
    }

    /// One safety copy's bytes (cswap's `_read_safety_copy`).
    pub fn safety_copy(&self, entry_id: &str) -> String {
        self.switcher.store.read_unclaimed_credential(entry_id).0
    }

    /// The only safety copy's id.
    pub fn only_stash_id(&self) -> String {
        let stash = self.stash();
        assert_eq!(stash.len(), 1, "{stash:?}");
        stash.keys().next().unwrap().clone()
    }
}

/// A profile answer, `None` fields left out (`organization` absent is cswap's
/// `organizationUuid: None`).
pub(super) fn profile(uuid: &str, email: Option<&str>, org: Option<&str>) -> Value {
    let mut account = json!({"uuid": uuid});
    if let Some(email) = email {
        account["email"] = email.into();
    }
    let mut body = json!({"account": account});
    if let Some(org) = org {
        body["organization"] = json!({"uuid": org});
    }
    body
}

/// An OAuth credential with these tokens.
pub(super) fn oauth_creds(access: &str, refresh: &str) -> String {
    json!({"claudeAiOauth": {"accessToken": access, "refreshToken": refresh}}).to_string()
}

/// An OAuth credential with an access token only.
pub(super) fn access_creds(access: &str) -> String {
    json!({"claudeAiOauth": {"accessToken": access}}).to_string()
}

/// Whether `dir` holds a temp file a write left behind.
pub(super) fn tmp_files_in(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
}

/// cswap's `sample_sequence_data`.
pub(super) fn sample_sequence_data() -> Value {
    json!({
        "activeAccountNumber": 1,
        "lastUpdated": "2024-01-01T00:00:00Z",
        "sequence": [1, 2],
        "accounts": {
            "1": {"email": "account1@example.com", "uuid": "uuid-1", "added": "2024-01-01T00:00:00Z"},
            "2": {"email": "account2@example.com", "uuid": "uuid-2", "added": "2024-01-02T00:00:00Z"},
        },
    })
}

/// HTTP that answers through a closure, which may change the world as a request is in flight.
pub(super) struct ScriptedHttp<F>(pub F);

impl<F> Http for ScriptedHttp<F>
where
    F: Fn(&Request) -> Result<Response, HttpError> + Send + Sync,
{
    fn send(&self, request: &Request) -> Result<Response, HttpError> {
        (self.0)(request)
    }
}

/// A slot the way cswap's `_seed` makes one: a credential and config backup, a record, in the
/// sequence; the first seeded slot active.
pub(super) fn seed(h: &Harness, num: u64, email: &str) {
    seed_with(h, num, email, true, true);
}

pub(super) fn seed_with(h: &Harness, num: u64, email: &str, creds: bool, config: bool) {
    let n = num.to_string();
    if creds {
        h.set_backup(
            &n,
            email,
            &oauth_creds(&format!("sk-{num}"), &format!("rt-{num}")),
        );
    }
    if config {
        h.set_config_backup(
            &n,
            email,
            &json!({"oauthAccount": {"emailAddress": email, "accountUuid": format!("uuid-{num}")}}),
        );
    }
    h.switcher.setup_directories().unwrap();
    h.switcher.init_sequence_file().unwrap();
    let mut data = h.switcher.get_sequence_data().unwrap().unwrap();
    data["accounts"].as_object_mut().unwrap().insert(
        n,
        json!({
            "email": email, "uuid": format!("uuid-{num}"),
            "organizationUuid": "", "organizationName": "", "added": "2024-01-01T00:00:00Z",
        }),
    );
    let sequence = data["sequence"].as_array_mut().unwrap();
    if !sequence.contains(&Value::from(num)) {
        sequence.push(Value::from(num));
        sequence.sort_by_key(Value::as_u64);
    }
    if data["activeAccountNumber"].is_null() {
        data["activeAccountNumber"] = Value::from(num);
    }
    h.write_sequence(&Value::Object(data));
}

/// cswap's `_make_live`: the live login pointed at a seeded slot.
pub(super) fn make_live(h: &Harness, email: &str, num: u64) {
    h.set_live(&oauth_creds(
        &format!("sk-live-{num}"),
        &format!("rt-live-{num}"),
    ));
    h.write_config(
        &json!({"oauthAccount": {"emailAddress": email, "accountUuid": format!("uuid-{num}")}}),
    );
}

#[test]
fn the_switcher_is_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<Switcher>();
}
