//! The control policy, kept in `settings.json` under `externalControl`: whether clients
//! outside Colosso's terminals may control the app, the port the MCP server listens on for
//! them, and the clients the person allowed, each with its scopes and last use. Only the
//! person changes it, in Settings, through the functions here; no operation reads or sets it.

use colosso_app_control::Scope;
use gpui_kit::App;
use serde::{Deserialize, Serialize};

use super::Control;
use super::activity::{ActivityEntry, ActivityKind, Decision, now_ms};
use crate::settings;

pub use colosso_term_mcp::http::DEFAULT_PORT;

/// Who may control Colosso from outside its terminals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    /// Whether external clients may control the app at all. Off until the person turns it on.
    pub enabled: bool,
    /// The MCP server's port on localhost.
    pub port: u16,
    /// The clients the person allowed, in the order they were allowed.
    pub clients: Vec<ApprovedClient>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: false,
            port: DEFAULT_PORT,
            clients: Vec::new(),
        }
    }
}

/// A client the person allowed to control Colosso.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovedClient {
    /// The name it says hello with.
    pub name: String,
    /// What it may do.
    pub scopes: Vec<Scope>,
    /// When the person allowed it, in milliseconds since the Unix epoch.
    pub approved_at: u64,
    /// When it last said hello, in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<u64>,
}

impl Policy {
    /// The allowed client called `name`.
    pub fn client(&self, name: &str) -> Option<&ApprovedClient> {
        self.clients.iter().find(|client| client.name == name)
    }

    fn client_mut(&mut self, name: &str) -> Option<&mut ApprovedClient> {
        self.clients.iter_mut().find(|client| client.name == name)
    }

    /// Allows `name` with the default grant, unless it's allowed already.
    fn approve(&mut self, name: &str, now: u64) {
        if self.client(name).is_none() {
            self.clients.push(ApprovedClient {
                name: name.to_owned(),
                scopes: Scope::DEFAULT_GRANT.to_vec(),
                approved_at: now,
                last_used: None,
            });
        }
    }
}

/// The policy as it stands.
pub fn policy(cx: &App) -> Policy {
    settings::get(cx).external_control.clone()
}

/// Turns external control on or off. Off, hellos and calls of external clients are
/// refused, and the ones waiting on the person with them.
#[cfg(test)]
pub fn set_enabled(enabled: bool, cx: &mut App) {
    settings::update(cx, |settings| settings.external_control.enabled = enabled);
}

/// Sets the MCP server's port.
pub fn set_port(port: u16, cx: &mut App) {
    settings::update(cx, |settings| settings.external_control.port = port);
}

/// Sets what the allowed client `name` may do; nothing when no such client is allowed.
pub fn set_client_scopes(name: &str, scopes: Vec<Scope>, cx: &mut App) {
    let mut scopes = scopes;
    scopes.sort_by_key(|scope| Scope::ALL.iter().position(|all| all == scope));
    scopes.dedup();
    settings::update(cx, |settings| {
        if let Some(client) = settings.external_control.client_mut(name) {
            client.scopes = scopes;
        }
    });
}

/// Takes back what client `name` was allowed, and forgets a denial of it: its next hello
/// asks the person again.
pub fn revoke(name: &str, cx: &mut App) {
    settings::update(cx, |settings| {
        settings
            .external_control
            .clients
            .retain(|client| client.name != name);
    });
    if cx.has_global::<Control>() {
        Control::get_mut(cx).denied.remove(name);
    }
}

/// Allows client `name`, denied lately, with the default grant, as the prompt's Allow would.
pub fn allow_denied(name: &str, cx: &mut App) {
    approve(name, cx);
    if cx.has_global::<Control>() {
        let control = Control::get_mut(cx);
        control.denied.remove(name);
        let kind = ActivityKind::Approval(Decision::Allowed);
        control.activity.record(name.to_owned(), kind);
    }
}

/// The clients the person denied lately, refused without asking until the denial passes.
pub fn denied(cx: &App) -> Vec<String> {
    let Some(control) = cx.try_global::<Control>() else {
        return Vec::new();
    };
    let mut names: Vec<String> = control.denied.keys().cloned().collect();
    names.sort();
    names
}

/// The latest `count` entries of the activity log, newest first.
pub fn activity(count: usize, cx: &App) -> Vec<ActivityEntry> {
    cx.try_global::<Control>()
        .map(|control| control.activity.entries(count))
        .unwrap_or_default()
}

/// Allows `name` with the default grant.
pub(crate) fn approve(name: &str, cx: &mut App) {
    let now = now_ms();
    settings::update(cx, |settings| settings.external_control.approve(name, now));
}

/// How stale a client's last use may get before a hello notes it again, so a client calling
/// often doesn't write the settings each time.
const TOUCH_EVERY_MS: u64 = 60_000;

/// Notes that the allowed client `name` said hello now.
pub(crate) fn touch(name: &str, cx: &mut App) {
    let now = now_ms();
    let fresh = (settings::get(cx).external_control.clients.iter())
        .find(|client| client.name == name)
        .and_then(|client| client.last_used)
        .is_some_and(|at| now.saturating_sub(at) < TOUCH_EVERY_MS);
    if fresh {
        return;
    }
    settings::update(cx, |settings| {
        if let Some(client) = settings.external_control.client_mut(name) {
            client.last_used = Some(now);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_settings_key_with_its_defaults() {
        let policy: Policy = serde_json::from_str(r#"{ "enabled": true }"#).unwrap();
        assert_eq!(
            policy,
            Policy {
                enabled: true,
                ..Policy::default()
            }
        );
        let written = serde_json::to_value(Policy {
            clients: vec![ApprovedClient {
                name: "Claude Code".into(),
                scopes: vec![Scope::Read],
                approved_at: 1,
                last_used: Some(2),
            }],
            ..Policy::default()
        })
        .unwrap();
        assert_eq!(
            written,
            serde_json::json!({
                "enabled": false,
                "port": 6272,
                "clients": [
                    { "name": "Claude Code", "scopes": ["read"], "approvedAt": 1, "lastUsed": 2 }
                ]
            })
        );
    }
}
