//! The MCP server page's clients: those the person allowed, each with when it was last used,
//! what it may do and Revoke; those denied lately, with Allow; and the latest activity:
//! every changing call, and every approval and confirmation.

use koyo_app_control::Scope;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, SharedString};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use crate::control::{self, ActivityEntry, ActivityKind, Decision};

/// How many of the latest activity entries the page lists.
const SHOWN_ACTIVITY: usize = 30;

/// Turns one of a client's scopes on or off, from its menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct ToggleClientScope {
    pub client: String,
    pub scope: Scope,
}

/// Answers the page's menu actions, wherever the menu was opened.
pub fn init(cx: &mut App) {
    cx.on_action(|toggle: &ToggleClientScope, cx| {
        let policy = control::policy(cx);
        let Some(client) = policy.client(&toggle.client) else {
            return;
        };
        let mut scopes = client.scopes.clone();
        if let Some(at) = scopes.iter().position(|scope| *scope == toggle.scope) {
            scopes.remove(at);
        } else {
            scopes.push(toggle.scope);
        }
        control::set_client_scopes(&toggle.client, scopes, cx);
    });
}

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    if !control::policy(cx).enabled {
        return Vec::new();
    }
    let mut sections = vec![clients(cx)];
    let denied = control::denied(cx);
    if !denied.is_empty() {
        sections.push(denied_clients(denied));
    }
    sections.push(activity(cx));
    sections
}

fn clients(cx: &App) -> SettingsSection {
    let policy = control::policy(cx);
    let section = SettingsSection::new("Clients");
    if policy.clients.is_empty() {
        return section.row(
            SettingRow::new("No clients yet")
                .description("A client asks once, in this window, the first time it connects."),
        );
    }
    section.rows(policy.clients.iter().map(|client| {
        let used = client.last_used.map_or_else(
            || "Never used".to_owned(),
            |at| format!("Last used {}", ago(at)),
        );
        let name = client.name.clone();
        let menu_name = client.name.clone();
        let revoked = client.name.clone();
        SettingRow::new(name.clone())
            .description(SharedString::from(used))
            .control(MenuButton::new(
                SharedString::from(format!("mcp-scopes-{name}")),
                scopes_label(&client.scopes),
                move |cx| scopes_menu(&menu_name, cx),
            ))
            .control(
                PillButton::new(SharedString::from(format!("mcp-revoke-{name}")), "Revoke")
                    .on_click(move |_, _, cx| control::revoke(&revoked, cx)),
            )
    }))
}

/// What a client may do, in a few words.
fn scopes_label(scopes: &[Scope]) -> String {
    if scopes.is_empty() {
        return "Nothing".to_owned();
    }
    if scopes.len() == Scope::ALL.len() {
        return "Everything".to_owned();
    }
    scopes
        .iter()
        .map(|scope| scope.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every scope, those `client` holds checked.
fn scopes_menu(client: &str, cx: &App) -> NativeMenu {
    let policy = control::policy(cx);
    let held = policy
        .client(client)
        .map(|client| client.scopes.clone())
        .unwrap_or_default();
    Scope::ALL
        .into_iter()
        .fold(NativeMenu::new(), |menu, scope| {
            menu.menu_with_check(
                scope.label(),
                held.contains(&scope),
                Box::new(ToggleClientScope {
                    client: client.to_owned(),
                    scope,
                }),
            )
        })
}

fn denied_clients(denied: Vec<String>) -> SettingsSection {
    SettingsSection::new("Denied lately").rows(denied.into_iter().map(|name| {
        let allowed = name.clone();
        SettingRow::new(name.clone())
            .description("Refused without asking for ten minutes after you denied it.")
            .control(
                PillButton::new(SharedString::from(format!("mcp-allow-{name}")), "Allow")
                    .on_click(move |_, _, cx| control::allow_denied(&allowed, cx)),
            )
    }))
}

fn activity(cx: &App) -> SettingsSection {
    let entries = control::activity(SHOWN_ACTIVITY, cx);
    let section = SettingsSection::new("Recent activity");
    if entries.is_empty() {
        return section.row(SettingRow::new("Nothing yet").description(
            "Calls that change something, and your answers to clients, are listed here.",
        ));
    }
    section.rows(entries.iter().map(activity_row))
}

fn activity_row(entry: &ActivityEntry) -> SettingRow {
    let (what, detail) = match &entry.kind {
        ActivityKind::Call { operation, outcome } => match outcome {
            Ok(summary) => (operation.clone(), summary.clone()),
            Err(error) => (
                operation.clone(),
                format!("{}: {}", error.code.id(), error.message),
            ),
        },
        ActivityKind::Approval(decision) => (
            "Asked to control Koyo".to_owned(),
            decision_label(*decision).to_owned(),
        ),
        ActivityKind::Confirmation {
            operation,
            question,
            decision,
        } => (
            operation.clone(),
            format!("{question} {}", decision_label(*decision)),
        ),
    };
    SettingRow::new(format!("{} · {what}", entry.client))
        .description(SharedString::from(format!("{} · {detail}", ago(entry.at))))
}

fn decision_label(decision: Decision) -> &'static str {
    match decision {
        Decision::Allowed => "Allowed.",
        Decision::Denied => "Denied.",
        Decision::TimedOut => "No answer.",
    }
}

/// How long ago `at`, in milliseconds since the Unix epoch, was.
fn ago(at: u64) -> String {
    let seconds = koyo_term_control::now_ms().saturating_sub(at) / 1000;
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_read_as_a_few_words() {
        assert_eq!(scopes_label(&[]), "Nothing");
        assert_eq!(scopes_label(&Scope::ALL), "Everything");
        assert_eq!(
            scopes_label(&[Scope::Read, Scope::Navigate]),
            "Read, Navigate"
        );
    }
}
