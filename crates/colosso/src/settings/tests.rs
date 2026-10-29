use gpui_kit::point;

use super::*;
use crate::gradient::Harmony;

#[test]
fn settings_survive_a_round_trip() {
    let mut settings = Settings {
        appearance: ThemePreference::Dark,
        layout: Layout::Arc,
        sidebar_width: 320.,
        changes_width: 700.,
        projects: vec!["/src/app".into()],
        worktrees_dir: Some("/work".into()),
        branch_prefix: BranchPrefix::Custom("me".into()),
        ..Settings::default()
    };
    settings.dark_theme.dots = vec![point(250., 120.), point(110., 240.)];
    settings.dark_theme.harmony = Harmony::Complementary;
    settings.light_theme.texture = 0.5;
    let json = serde_json::to_vec(&settings).unwrap();
    assert_eq!(serde_json::from_slice::<Settings>(&json).unwrap(), settings);
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "a missing width is the default itself, not near it"
)]
fn missing_fields_fall_back_to_defaults() {
    let settings: Settings = serde_json::from_str(r#"{ "layout": "Arc" }"#).unwrap();
    assert_eq!(settings.layout, Layout::Arc);
    assert_eq!(settings.sidebar_width, theme::SIDEBAR_WIDTH);
    assert_eq!(settings.changes_width, theme::CHANGES_WIDTH);
    assert_eq!(settings.dark_theme, GradientTheme::default());
    assert_eq!(settings.branch_prefix, BranchPrefix::GitHubUser);
}

#[gpui_kit::test]
fn edits_to_the_file_are_taken_up_while_the_app_runs(cx: &mut gpui_kit::TestAppContext) {
    let path = crate::test_support::scratch("settings-reload").join(FILE);
    let store = Store {
        settings: Settings::default(),
        path: Some(path.clone()),
        pending: None,
        unsaved: false,
        seen: None,
    };
    cx.update(|cx| cx.set_global(store));
    let mut edited = SystemTime::UNIX_EPOCH;
    let mut edit = |json: &str| {
        std::fs::write(&path, json).unwrap();
        edited += Duration::from_secs(1);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(edited).unwrap();
    };
    let delay = |cx: &mut gpui_kit::TestAppContext| cx.read(|cx| get(cx).key_repeat.delay_ms);
    edit(r#"{ "key_repeat": { "delay_ms": 90 } }"#);
    cx.update(reload);
    assert_eq!(delay(cx), 90);
    // Half written, it's left until the next edit.
    edit(r#"{ "key_repeat": { "delay_ms": 4"#);
    cx.update(reload);
    assert_eq!(delay(cx), 90);
    // A change made in the app and not written yet wins over the file.
    cx.update(|cx| update(cx, |settings| settings.key_repeat.delay_ms = 120));
    edit(r#"{ "key_repeat": { "delay_ms": 60 } }"#);
    cx.update(reload);
    assert_eq!(delay(cx), 120);
}

#[test]
fn a_file_with_retired_keys_reads_without_them() {
    let path = crate::test_support::scratch("settings-migrate").join(FILE);
    let old = r#"{
        "layout": "Arc",
        "files_width": 280,
        "key_repeat": {
            "delay_ms": 200,
            "start_per_frame": 0.8,
            "accelerate_after_ms": 500,
            "full_speed_after_ms": 1500,
            "most_per_frame": 30.0
        }
    }"#;
    std::fs::write(&path, old).unwrap();
    let mut value = serde_json::from_str(old).unwrap();
    migrate(&mut value);
    assert!(value.get("files_width").is_none());
    assert_eq!(value["key_repeat"], serde_json::json!({ "delay_ms": 200 }));
    let settings = read(&path).unwrap();
    assert_eq!(settings.layout, Layout::Arc);
    let repeat = KeyRepeat {
        delay_ms: 200,
        ..KeyRepeat::default()
    };
    assert_eq!(settings.key_repeat, repeat);
}

#[test]
fn worktrees_go_in_a_folder_per_project() {
    let settings = Settings {
        worktrees_dir: Some("/work".into()),
        ..Settings::default()
    };
    assert_eq!(settings.worktrees_dir("app"), Path::new("/work/app"));
    assert!(
        Settings::default()
            .worktrees_dir("app")
            .ends_with("colosso/workspaces/app")
    );
}
