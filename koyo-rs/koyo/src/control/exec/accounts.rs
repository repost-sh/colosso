//! `claude_accounts_list`: the accounts as the sidebar's panel shows them (`accounts/`), each
//! window as of now: a weekly window past its reset is empty again, its reset a week on.

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::accounts::{
    AccountProblem, AccountWindow, ClaudeAccount, ClaudeAccountsListOutput, ProblemKind,
};
use serde_json::Value;

use super::output;
use crate::accounts::{self, Problem, Row};
use crate::settings;

/// Seconds since the epoch as the operation's milliseconds; before it, 0.
fn ms(at: f64) -> u64 {
    (at * 1000.) as u64
}

pub(crate) fn list(cx: &App) -> Result<Value, ControlError> {
    let now = accounts::now();
    let view = accounts::view(cx);
    output(&ClaudeAccountsListOutput {
        managed: settings::get(cx).accounts.manage,
        accounts: view.rows.iter().map(|row| account(row, now)).collect(),
        unreadable: view.unreadable.clone(),
        overridden_by: view.overridden_by.map(str::to_owned),
    })
}

/// `row` as agents read it, its windows as of `now`.
fn account(row: &Row, now: f64) -> ClaudeAccount {
    ClaudeAccount {
        email: row.email.to_string(),
        organisation: row.organisation.as_ref().map(ToString::to_string),
        active: row.active,
        windows: (row.bars.iter())
            .map(|bar| {
                let bar = bar.as_of(now);
                AccountWindow {
                    name: bar.label.to_string(),
                    percent: bar.percent,
                    resets_at: bar.resets_at.map(ms),
                    ahead_of_pace: bar.ahead_of_pace,
                }
            })
            .collect(),
        read_at: row.measured_at.map(ms),
        problem: row.problem.as_ref().map(|problem| AccountProblem {
            kind: kind(problem),
            message: problem.sentence(now),
            until: match problem {
                Problem::RateLimited { until } => Some(ms(*until)),
                _ => None,
            },
        }),
    }
}

fn kind(problem: &Problem) -> ProblemKind {
    match problem {
        Problem::RateLimited { .. } => ProblemKind::RateLimited,
        Problem::PlanLapsed => ProblemKind::PlanLapsed,
        Problem::CertificateRefused => ProblemKind::CertificateRefused,
        Problem::SignInAgain => ProblemKind::SignInAgain,
        Problem::TokenExpired => ProblemKind::TokenExpired,
        Problem::KeychainUnavailable => ProblemKind::KeychainUnavailable,
        Problem::ForeignLogin => ProblemKind::ForeignLogin,
        Problem::Offline => ProblemKind::Offline,
        Problem::Other(_) => ProblemKind::Other,
    }
}
