//! Settings → Accounts (`docs/claude-accounts.md` 1, 5, 6): whether Koyo manages the Mac's
//! Claude Code login, the accounts it keeps, each with the way to hold it out of auto-switch or
//! remove it, and auto-switch with its threshold. Nothing here is reachable by `settings_set` or
//! any agent's tool: only the person changes how their login is managed.

use gpui_kit::{App, SharedString};
use koyo_accounts::autoswitch::{THRESHOLD_MAX, THRESHOLD_MIN};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::accounts::{self, Job, Row, words};
use crate::settings::{self, SetAccountsThreshold, checked_menu};

/// The thresholds the menu offers, the range's ends among them.
const THRESHOLDS: [f64; 11] = [
    THRESHOLD_MIN,
    60.,
    70.,
    75.,
    80.,
    85.,
    90.,
    95.,
    97.,
    99.,
    THRESHOLD_MAX,
];

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let mut sections = vec![managing()];
    if settings::get(cx).accounts.manage {
        sections.push(kept(cx));
        sections.push(auto_switch(cx));
    }
    sections
}

fn managing() -> SettingsSection {
    SettingsSection::new("Claude accounts").row(
        SettingRow::new("Manage Claude accounts")
            .description(
                "Koyo keeps your Claude Code logins and makes one of them the Mac's login, so \
                 every Claude Code on the Mac follows: Koyo's chats and terminals, other \
                 terminals, VS Code. Off, Koyo never reads or changes the login. Turn it off if \
                 claude-swap or another tool manages your accounts: two managers on one login \
                 fight over its refresh token.",
            )
            .control(NativeSwitch::new(
                "accounts-manage",
                |settings| settings.accounts.manage,
                |settings, on| settings.accounts.manage = on,
            )),
    )
}

/// An account's row: where it stands, Hold Out or Put Back, and Remove.
fn account(row: &Row) -> SettingRow {
    let (held, removed, disabled) = (row.slot(), row.slot(), row.disabled);
    SettingRow::new(row.email.clone())
        .description(row.under())
        .control(
            PillButton::new(
                SharedString::from(format!("accounts-hold-{}", row.number)),
                if disabled { "Put Back" } else { "Hold Out" },
            )
            .on_click(move |_, _, cx| {
                let job = Job::Hold(held.clone(), !disabled);
                accounts::accounts(cx).update(cx, |accounts, cx| accounts.ask(job, cx));
            }),
        )
        .control(
            PillButton::new(
                SharedString::from(format!("accounts-remove-{}", row.number)),
                "Remove",
            )
            .on_click(move |_, window, cx| accounts::ask_remove(removed.clone(), window, cx)),
        )
}

/// The accounts Koyo keeps, and Add Current Login.
fn kept(cx: &App) -> SettingsSection {
    let view = accounts::view(cx);
    let said = (view.said.clone())
        .or_else(|| view.doing.clone())
        .unwrap_or_else(|| {
            let mac = (view.unmanaged.as_deref())
                .map_or_else(String::new, |email| words::unmanaged(email) + " ");
            format!(
                "{mac}Keeps the Mac's Claude Code login as an account. To add another, sign in \
                 to it with Claude Code (/login), then add it here."
            )
        });
    SettingsSection::new("Accounts")
        .rows(view.rows.iter().map(account))
        .row(
            SettingRow::new("Add the current login")
                .description(said)
                .disabled(view.doing.is_some())
                .control(
                    PillButton::new("accounts-add", "Add Current Login")
                        .disabled(view.doing.is_some())
                        .on_click(|_, _, cx| {
                            let accounts = accounts::accounts(cx);
                            accounts.update(cx, |accounts, cx| accounts.ask(Job::Add, cx));
                        }),
                ),
        )
}

/// Auto-switch and its threshold; claude-swap's other numbers keep its defaults, unshown.
fn auto_switch(cx: &App) -> SettingsSection {
    let wanted = &settings::get(cx).accounts;
    let threshold = wanted.threshold();
    SettingsSection::new("Auto-switch")
        .row(
            SettingRow::new("Auto-switch")
                .description(
                    "When the Mac's login reaches the threshold on any window, or its limit, \
                     Koyo moves it to the account whose weekly limit resets soonest, as \
                     claude-swap's weekly-first: it waits 5 minutes between switches, lands only \
                     with room to spare, and skips accounts held out.",
                )
                .control(NativeSwitch::new(
                    "accounts-auto-switch",
                    |settings| settings.accounts.auto_switch,
                    |settings, on| settings.accounts.auto_switch = on,
                )),
        )
        .row(
            SettingRow::new("Threshold")
                .description("The usage at which auto-switch moves on.")
                .disabled(!wanted.auto_switch)
                .control(MenuButton::new(
                    "accounts-threshold",
                    words::threshold(threshold),
                    move |_| {
                        checked_menu(THRESHOLDS, threshold, words::threshold, |choice| {
                            Box::new(SetAccountsThreshold(choice))
                        })
                    },
                )),
        )
}
