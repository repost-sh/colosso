//! The settings an operation reads and sets: the allowlist only, never the projects or the
//! control policy, and nothing changed by a call refused in part.

use gpui_kit::TestAppContext;
use koyo_app_control::{Command, ErrorCode};
use serde_json::json;

use super::{call, controlled, terminal};
use crate::settings::{
    self, AddressBar, BranchPrefix, ChangesAndFiles, LoosePages, SaveScope, Tabs,
};
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
    assert_eq!(read["loosePages"], json!("shared"));
    let set = json!({
        "appearance": "dark",
        "previewFiles": false,
        "saveScope": "all",
        "loosePages": "per_space",
        "branchPrefix": { "custom": " fs " },
        "sidebarWidth": 300,
        "mostLanguageServers": null,
        "devtools": { "dock": "right" },
    });
    let after = call(&agent, "settings_set", set, cx).unwrap();
    assert_eq!(after["appearance"], json!("dark"));
    assert_eq!(after["branchPrefix"], json!({ "custom": "fs" }));
    assert_eq!(after["devtools"]["dock"], json!("right"));
    assert_eq!(after["loosePages"], json!("per_space"));
    cx.read(|cx| {
        let settings = settings::get(cx);
        assert_eq!(settings.appearance, ThemePreference::Dark);
        assert!(!settings.preview_files);
        assert_eq!(settings.save_scope, SaveScope::All);
        assert_eq!(settings.loose_pages, LoosePages::PerSpace);
        assert_eq!(settings.branch_prefix, BranchPrefix::Custom("fs".into()));
        assert!((settings.sidebar_width - 300.).abs() < f32::EPSILON);
    });
    let reset = call(&agent, "settings_set", json!({ "worktreesDir": null }), cx).unwrap();
    assert_eq!(reset["worktreesDir"], json!(null));
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

/// The tabs, the address bar and the Changes and Files buttons read and move as the Layout
/// settings do, and the buttons' top rail is kept while the tabs in the sidebar leave none.
#[gpui_kit::test]
fn settings_set_moves_the_tabs_the_address_bar_and_changes_and_files(cx: &mut TestAppContext) {
    let root = repo("control-placement", &[("README.md", "hi\n")]);
    let _rig = controlled("control-placement", &[&root], cx);
    let agent = terminal("term_1");
    let read = call(&agent, "settings_get", json!({}), cx).unwrap();
    let defaults = json!({
        "tabs": "top_rail",
        "addressBar": "sidebar",
        "changesAndFiles": "top_rail",
    });
    for (key, value) in defaults.as_object().unwrap() {
        assert_eq!(&read[key], value, "{key}");
    }
    let set = json!({ "tabs": "sidebar", "addressBar": "page", "changesAndFiles": "worktree" });
    let after = call(&agent, "settings_set", set.clone(), cx).unwrap();
    for (key, value) in set.as_object().unwrap() {
        assert_eq!(&after[key], value, "{key}");
    }
    let placed = |cx: &mut TestAppContext| {
        cx.read(|cx| {
            let settings = settings::get(cx);
            (settings.tabs, settings.address_bar, settings.changes_and_files)
        })
    };
    assert_eq!(
        placed(cx),
        (Tabs::Sidebar, AddressBar::Page, ChangesAndFiles::Worktree)
    );
    let kept = json!({ "changesAndFiles": "top_rail" });
    let after = call(&agent, "settings_set", kept, cx).unwrap();
    assert_eq!(after["changesAndFiles"], json!("top_rail"));
    assert_eq!(placed(cx).2, ChangesAndFiles::TopRail, "kept for the top rail");
    let shown = cx.read(|cx| settings::get(cx).changes_and_files_shown());
    assert_eq!(shown, ChangesAndFiles::Worktree, "on the row while the tabs are in the sidebar");
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
