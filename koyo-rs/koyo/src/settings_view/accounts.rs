//! Settings → Accounts (`docs/claude-accounts.md` 1, 6): whether Koyo manages the Mac's Claude
//! Code login. Nothing here is reachable by `settings_set`: only the person changes how their
//! login is managed.

use gpui_kit::App;

use super::components::{SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;

pub fn sections(_: &App) -> Vec<SettingsSection> {
    vec![managing()]
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
