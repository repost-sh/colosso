//! The settings an operation reads and sets: the allowlist only, never the projects or the
//! control policy, and nothing changed by a call refused in part.

use colosso_app_control::{Command, ErrorCode};
use gpui_kit::TestAppContext;
use serde_json::json;

use super::{call, controlled, terminal};
use crate::settings::{self, BranchPrefix, SaveScope};
use crate::test_support::repo;
use crate::theme::ThemePreference;

#[gpui_kit::test]
fn settings_set_changes_the_allowlisted_settings_only(cx: &mut TestAppContext) {
    let root = repo("control-settings", &[("README.md", "hi\n")]);
    let rig = controlled("control-settings", &[&root], cx);
    let agent = terminal("term_1");
    let read = call(&agent, "settings_get", json!({}), cx).unwrap();
    let keys: Vec<&str> = read
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(
        !keys.contains(&"projects") && !keys.contains(&"externalControl"),
        "{keys:?}"
    );
    assert_eq!(read["worktreesDir"], json!(rig.worktrees));
    let set = json!({
        "appearance": "dark",
        "previewFiles": false,
        "saveScope": "all",
        "branchPrefix": { "custom": " fs " },
        "sidebarWidth": 300,
        "mostLanguageServers": null,
        "devtools": { "dock": "right" },
    });
    let after = call(&agent, "settings_set", set, cx).unwrap();
    assert_eq!(after["appearance"], json!("dark"));
    assert_eq!(after["branchPrefix"], json!({ "custom": "fs" }));
    assert_eq!(after["devtools"]["dock"], json!("right"));
    cx.read(|cx| {
        let settings = settings::get(cx);
        assert_eq!(settings.appearance, ThemePreference::Dark);
        assert!(!settings.preview_files);
        assert_eq!(settings.save_scope, SaveScope::All);
        assert_eq!(settings.branch_prefix, BranchPrefix::Custom("fs".into()));
        assert!((settings.sidebar_width - 300.).abs() < f32::EPSILON);
    });
    // The worktrees folder back to its default.
    let reset = call(&agent, "settings_set", json!({ "worktreesDir": null }), cx).unwrap();
    assert_eq!(reset["worktreesDir"], json!(null));
    // A field out of bounds refuses the whole call.
    let refused = call(
        &agent,
        "settings_set",
        json!({ "appearance": "light", "sidebarWidth": 10 }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidInput);
    assert_eq!(
        cx.read(|cx| settings::get(cx).appearance),
        ThemePreference::Dark
    );
    let refused = call(
        &agent,
        "settings_set",
        json!({ "worktreesDir": "relative/dir" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidInput);
}

#[test]
fn the_projects_and_the_policy_are_out_of_reach() {
    for input in [
        json!({ "projects": ["/somewhere"] }),
        json!({ "externalControl": { "enabled": true } }),
        json!({ "languageServers": {} }),
    ] {
        let refused = Command::parse("settings_set", input.clone()).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{input}");
    }
}
