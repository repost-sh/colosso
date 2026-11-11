//! Claude accounts (`docs/claude-accounts.md` 6, `docs/app-control.md` 5): the Mac's Claude
//! Code login as Koyo shows it, and how much of its week is used. Listing it is a read;
//! switching stays with the person.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{NoInput, count};
use crate::{Hints, Operation, Scope};

/// One window of an account's usage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountWindow {
    /// "7 days", or the model whose own weekly limit it is ("Fable").
    pub name: String,
    /// Percent used, or null when Claude's usage page didn't say.
    pub percent: Option<f64>,
    /// When it resets, in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    /// Whether it's used faster than its time passes.
    pub ahead_of_pace: bool,
}

/// What keeps an account's usage from being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    /// Claude's usage page asked to wait, until `until`.
    RateLimited,
    /// The organisation doesn't allow signing in: a lapsed plan.
    PlanLapsed,
    /// The TLS certificate was refused: a proxy that re-signs traffic, a portal, the clock.
    CertificateRefused,
    /// The login has ended; the person signs in again.
    SignInAgain,
    /// The access token expired; Claude Code renews it with its next message.
    TokenExpired,
    /// The Keychain was locked or busy; still signed in.
    KeychainUnavailable,
    Offline,
    Other,
}

/// What keeps an account's usage from being read, with the remedy said.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountProblem {
    pub kind: ProblemKind,
    /// What the person is told, with its remedy.
    pub message: String,
    /// For `rate_limited`: when it may be read again, in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
}

/// One account Koyo shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organisation: Option<String>,
    /// Whether it's the Mac's Claude Code login now.
    pub active: bool,
    /// The 7 days first, then each model's own weekly limit, as last read.
    pub windows: Vec<AccountWindow>,
    /// When its usage was last read, in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_at: Option<u64>,
    /// What keeps its usage from being read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<AccountProblem>,
}

/// The accounts `claude_accounts_list` found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccountsListOutput {
    /// Whether Koyo manages the Mac's Claude Code login (Settings → Accounts); off, it lists
    /// nothing.
    pub managed: bool,
    pub accounts: Vec<ClaudeAccount>,
    /// Why the login couldn't be read at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
    /// A variable set in the login shell (`ANTHROPIC_API_KEY`, …) that makes Claude Code there
    /// use it rather than the Mac's login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overridden_by: Option<String>,
}

/// `claude_accounts_list`: the Claude Code accounts Koyo keeps and their usage.
#[derive(Debug)]
pub struct ClaudeAccountsList;

impl Operation for ClaudeAccountsList {
    const NAME: &'static str = "claude_accounts_list";
    const TITLE: &'static str = "List Claude accounts";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The Claude Code accounts Koyo shows in the sidebar (Settings → Accounts): each one's email and organisation, whether it's the Mac's login now (`active`), and its usage as last read: the 7-day window and each model's own weekly window, in percent, with when each resets and when it was read, or what keeps it from being read (`problem.kind` with its remedy in `message`). `unreadable` says why the login couldn't be read at all; `overriddenBy` names a shell variable that makes Claude Code ignore the login. No tool changes the accounts: switching, adding, removing and auto-switch are the person's.";
    const HINTS: Hints = Hints::READ;
    type Input = NoInput;
    type Output = ClaudeAccountsListOutput;

    fn summary(output: &ClaudeAccountsListOutput) -> String {
        if !output.managed {
            return "Koyo doesn't manage Claude accounts".to_owned();
        }
        let listed = count(output.accounts.len(), "Claude account", "Claude accounts");
        match output.accounts.iter().find(|account| account.active) {
            Some(active) => format!("{listed}, {} active", active.email),
            None => listed,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_account_carries_only_what_it_knows() {
        let output = ClaudeAccountsListOutput {
            managed: true,
            accounts: vec![ClaudeAccount {
                email: "ana@example.com".into(),
                organisation: None,
                active: true,
                windows: vec![AccountWindow {
                    name: "7 days".into(),
                    percent: None,
                    resets_at: Some(1_800_000_000_000),
                    ahead_of_pace: false,
                }],
                read_at: None,
                problem: Some(AccountProblem {
                    kind: ProblemKind::RateLimited,
                    message: "Rate-limited.".into(),
                    until: Some(1_800_000_000_000),
                }),
            }],
            unreadable: None,
            overridden_by: None,
        };
        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            json!({
                "managed": true,
                "accounts": [{
                    "email": "ana@example.com", "active": true,
                    "windows": [{ "name": "7 days", "percent": null, "resetsAt": 1_800_000_000_000_u64, "aheadOfPace": false }],
                    "problem": { "kind": "rate_limited", "message": "Rate-limited.", "until": 1_800_000_000_000_u64 }
                }]
            })
        );
        assert_eq!(
            ClaudeAccountsList::summary(&output),
            "1 Claude account, ana@example.com active"
        );
    }
}
