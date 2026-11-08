//! The control policy, kept in `settings.json` under `externalControl`: whether clients
//! outside Koyo's terminals may control the app, the port the MCP server listens on for
//! them, the clients the person allowed, each with its scopes and last use, whether what any
//! agent opens is shown when its call doesn't say, and the size a page an agent opens starts
//! at. Only the person changes it, in Settings, through the functions here; no operation
//! reads or sets it.

use gpui_kit::App;
use koyo_app_control::Scope;
use serde::{Deserialize, Serialize};

use super::Control;
use super::activity::{ActivityEntry, ActivityKind, Decision, now_ms};
use crate::settings;

pub use koyo_term_mcp::http::DEFAULT_PORT;

/// Who may control Koyo from outside its terminals, and what any agent's call does to the
/// window when it doesn't say.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    /// Whether external clients may control the app at all. Off until the person turns it on.
    pub enabled: bool,
    /// The MCP server's port on localhost.
    pub port: u16,
    /// The clients the person allowed, in the order they were allowed.
    pub clients: Vec<ApprovedClient>,
    /// Whether what an agent makes or opens (a project, a worktree, a tab, a split, a file,
    /// the changes, a chat) is shown, the window switching to it, when the call leaves
    /// `select` out. Off until the person turns it on: it's listed in the sidebar and the tab
    /// strip, and the window stays where it was. A call that says `select` gets what it says.
    /// It holds for every caller, agents in Koyo's terminals too.
    pub show_what_agents_open: bool,
    /// What a browser page an agent opens starts at when its call leaves `viewport` out:
    /// responsive mode at a size, 1440 × 900 until the person changes it, or filling its
    /// pane as the person's own pages do. Not remembered for the page past the app's run.
    pub pages_open_at: PageSize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            enabled: false,
            port: DEFAULT_PORT,
            clients: Vec::new(),
            show_what_agents_open: false,
            pages_open_at: PageSize::AGENT,
        }
    }
}

/// The size a browser page starts at: `"fill"`, filling its pane, or `{ "width", "height" }`
/// in CSS pixels, in responsive mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSize {
    Fill,
    #[serde(untagged)]
    Size {
        width: u32,
        height: u32,
    },
}

impl PageSize {
    /// A desktop's, 1440 × 900: what pages agents open start at by default, as the tool
    /// descriptions say.
    pub const AGENT: PageSize = PageSize::Size {
        width: 1440,
        height: 900,
    };

    /// Its width and height, unless it fills the pane.
    pub fn size(self) -> Option<(u32, u32)> {
        match self {
            PageSize::Fill => None,
            PageSize::Size { width, height } => Some((width, height)),
        }
    }
}

/// A client the person allowed to control Koyo.
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

/// The size a page an agent opens starts at when its call leaves `viewport` out.
pub fn pages_open_at(cx: &App) -> PageSize {
    settings::get(cx).external_control.pages_open_at
}

/// Whether a call that makes or opens something shows it: as its `select` says, else as the
/// person set what agents open to show.
pub(crate) fn shows(select: Option<bool>, cx: &App) -> bool {
    select.unwrap_or_else(|| settings::get(cx).external_control.show_what_agents_open)
}

/// Sets the size a page an agent opens starts at, its call leaving it out.
pub fn set_pages_open_at(size: PageSize, cx: &mut App) {
    settings::update(cx, |settings| {
        settings.external_control.pages_open_at = size;
    });
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

/// The activity log, newest entry first.
pub fn activity(cx: &App) -> impl Iterator<Item = &ActivityEntry> {
    (cx.try_global::<Control>())
        .into_iter()
        .flat_map(|control| control.activity.newest())
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
                ],
                "showWhatAgentsOpen": false,
                "pagesOpenAt": { "width": 1440, "height": 900 }
            })
        );
    }

    #[test]
    fn pages_agents_open_start_at_a_size_or_fill_their_pane() {
        assert_eq!(Policy::default().pages_open_at.size(), Some((1440, 900)));
        let read = |json| {
            serde_json::from_value::<Policy>(json)
                .unwrap()
                .pages_open_at
        };
        assert_eq!(read(serde_json::json!({})), PageSize::AGENT);
        assert_eq!(
            read(serde_json::json!({ "pagesOpenAt": "fill" })),
            PageSize::Fill
        );
        assert_eq!(
            read(serde_json::json!({ "pagesOpenAt": { "width": 1920, "height": 1080 } })),
            PageSize::Size {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(PageSize::Fill.size(), None);
        assert_eq!(
            serde_json::to_value(PageSize::Fill).unwrap(),
            serde_json::json!("fill")
        );
        for wrong in [
            serde_json::json!({ "pagesOpenAt": "half" }),
            serde_json::json!({ "pagesOpenAt": { "width": 1440 } }),
        ] {
            assert!(
                serde_json::from_value::<Policy>(wrong.clone()).is_err(),
                "{wrong}"
            );
        }
    }
}
