//! The Claude accounts are the person's (`docs/claude-accounts.md` 6): agents read them, and no
//! operation switches, adds, removes, holds one out, turns auto-switch or Manage Claude accounts
//! on or off.

use koyo_app_control::{Command, Scope, infos};
use serde_json::json;

#[test]
fn no_operation_changes_the_claude_accounts() {
    let accounts: Vec<_> = (infos().into_iter())
        .filter(|info| info.name.contains("account"))
        .map(|info| (info.name, info.scope))
        .collect();
    assert_eq!(accounts, [("claude_accounts_list", Scope::Read)]);
    let refused = Command::parse("settings_set", json!({ "accounts": { "manage": false } }));
    assert!(refused.unwrap_err().message.contains("unknown field"));
}
