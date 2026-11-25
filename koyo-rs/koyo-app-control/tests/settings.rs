//! Where the tabs, the address bar and the Changes and Files buttons go, as `settings_set`
//! reads them and writes them back: by the names `docs/app-control.md` 5 gives them.

use koyo_app_control::ops::settings::{AddressBar, ChangesAndFiles, SettingsSetInput, Tabs};
use koyo_app_control::{Command, ErrorCode};
use serde_json::{Value, json};

/// `input` as `settings_set` reads it.
#[cfg(test)]
fn parse(input: Value) -> SettingsSetInput {
    let Command::SettingsSet(input) = Command::parse("settings_set", input).unwrap() else {
        panic!("settings_set parsed as another command");
    };
    input
}

#[test]
fn the_tabs_the_address_bar_and_changes_and_files_go_by_their_names() {
    let moved = json!({ "tabs": "sidebar", "addressBar": "page", "changesAndFiles": "worktree" });
    let input = parse(moved.clone());
    assert_eq!(
        (input.tabs, input.address_bar, input.changes_and_files),
        (
            Some(Tabs::Sidebar),
            Some(AddressBar::Page),
            Some(ChangesAndFiles::Worktree)
        )
    );
    assert_eq!(serde_json::to_value(&input).unwrap(), moved);
    let back = parse(json!({
        "tabs": "top_rail",
        "addressBar": "sidebar",
        "changesAndFiles": "top_rail",
    }));
    assert_eq!(
        (back.tabs, back.address_bar, back.changes_and_files),
        (
            Some(Tabs::TopRail),
            Some(AddressBar::Sidebar),
            Some(ChangesAndFiles::TopRail)
        )
    );
    assert_eq!(parse(json!({})).tabs, None, "left out, they stay");
    for (key, value) in [
        ("tabs", "TopRail"),
        ("addressBar", "In the page"),
        ("changesAndFiles", "rail"),
    ] {
        let refused = Command::parse("settings_set", json!({ key: value })).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{key}: {value}");
    }
}
