use gpui_kit::point;

use super::file::{read, reload};
use super::migration::migrate;
use super::*;
use crate::gradient::Harmony;

#[test]
fn settings_survive_a_round_trip() {
    let mut settings = Settings {
        appearance: ThemePreference::Dark,
        layout: Layout::Arc,
        sidebar_width: 320.,
        changes_files_width: 400.,
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
    assert_eq!(settings.changes_files_width, theme::CHANGES_FILES_WIDTH);
    assert_eq!(settings.dark_theme, GradientTheme::default());
    assert_eq!(settings.branch_prefix, BranchPrefix::GitHubUser);
    // As Zed and VS Code have them: previews on, no limit to the language servers, and none
    // stopped for being idle.
    assert!(settings.preview_files);
    assert!(settings.inlay_hints);
    assert_eq!(settings.most_language_servers, None);
    assert!(!settings.stop_idle_language_servers);
    assert!(settings.runs_server("rust-analyzer", None));
    // The composer's edge shows a chat working with a subtle line.
    assert_eq!(settings.chat_activity, ChatActivity::Subtle);
    // A markdown file opens as it reads, as GitHub shows it.
    assert_eq!(
        settings.markdown_opens_as,
        crate::files::MarkdownMode::Preview
    );
    let limited: Settings = serde_json::from_str(r#"{ "most_language_servers": 2 }"#).unwrap();
    assert_eq!(limited.most_language_servers, Some(2));
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
        },
        "language_servers": {
            "rust-analyzer": { "enabled": false },
            "vtsls": { "enabled": false, "off_in": ["/old/project"] }
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
    // The TypeScript switch keeps what was set under vtsls's name; the servers that came
    // since are listed, on.
    let servers: Vec<_> = settings
        .language_servers
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(servers, ["pyrefly", "rust-analyzer", "typescript"]);
    assert!(!settings.runs_server("rust-analyzer", None));
    assert!(!settings.runs_server("typescript", None));
    assert_eq!(
        settings.language_servers["typescript"].off_in,
        [PathBuf::from("/old/project")]
    );
    assert!(settings.runs_server("pyrefly", None));
}

#[test]
fn what_work_showed_becomes_the_verbosity_that_shows_as_much() {
    for (work, preset) in [
        ("Turns", Preset::Quiet),
        ("Steps", Preset::Normal),
        ("Everything", Preset::Verbose),
    ] {
        let mut value = serde_json::json!({ "chat_work": work });
        migrate(&mut value);
        assert!(value.get("chat_work").is_none());
        let settings: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(settings.chat_verbosity, preset.verbosity(), "{work}");
    }
    let chosen = Preset::Quiet.verbosity().with(Step::Thinking, Shows::All);
    let mut value = serde_json::json!({ "chat_work": "Everything", "chat_verbosity": chosen });
    migrate(&mut value);
    let settings: Settings = serde_json::from_value(value).unwrap();
    assert_eq!(settings.chat_verbosity, chosen, "what's chosen since stays");
}

#[test]
fn a_chat_text_size_picked_before_sizes_came_down_a_notch_keeps_its_place_on_the_slider() {
    for (old, new) in [(13., 11.), (16., 14.), (18., 15.), (22., 18.), (40., 18.)] {
        let mut value = serde_json::json!({ "fonts": { "chat_size": old, "code": "Menlo" } });
        migrate(&mut value);
        let settings: Settings = serde_json::from_value(value).unwrap();
        assert_eq!(
            settings.fonts.chat_size().to_bits(),
            f32::to_bits(new),
            "{old}"
        );
        assert_eq!(settings.fonts.code.as_deref(), Some("Menlo"));
    }
    let mut value = serde_json::json!({ "fonts": { "chat_text_size": 12. } });
    migrate(&mut value);
    let settings: Settings = serde_json::from_value(value).unwrap();
    assert_eq!(
        settings.fonts.chat_size().to_bits(),
        12_f32.to_bits(),
        "a new size stays"
    );
}

#[test]
fn what_the_composer_shows_while_a_chat_works_is_kept() {
    for activity in ChatActivity::ALL {
        let settings = Settings {
            chat_activity: activity,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["chat_activity"], format!("{activity:?}"));
        assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), settings);
    }
    let off: Settings = serde_json::from_str(r#"{ "chat_activity": "Off" }"#).unwrap();
    assert_eq!(off.chat_activity, ChatActivity::Off);
}

#[test]
fn prompts_float_until_a_person_turns_it_off_which_is_kept() {
    assert!(Settings::default().floating_prompts, "on by default");
    let missing: Settings = serde_json::from_str("{}").unwrap();
    assert!(
        missing.floating_prompts,
        "a file from before has them float"
    );
    let off = Settings {
        floating_prompts: false,
        ..Settings::default()
    };
    let json = serde_json::to_value(&off).unwrap();
    assert_eq!(json["floating_prompts"], false);
    assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), off);
}

#[test]
fn the_pinned_tabs_zone_shows_until_a_person_puts_it_away_which_is_kept() {
    assert!(!Settings::default().pinned_zone_dismissed, "shown at first");
    let missing: Settings = serde_json::from_str("{}").unwrap();
    assert!(
        !missing.pinned_zone_dismissed,
        "a file from before shows it"
    );
    let away = Settings {
        pinned_zone_dismissed: true,
        ..Settings::default()
    };
    let json = serde_json::to_value(&away).unwrap();
    assert_eq!(json["pinned_zone_dismissed"], true);
    assert_eq!(serde_json::from_value::<Settings>(json).unwrap(), away);
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
            .ends_with("koyo/workspaces/app")
    );
}

#[test]
fn a_language_server_is_turned_off_and_on_everywhere_or_in_a_project() {
    let (app, other) = (Path::new("/src/app"), Path::new("/src/other"));
    let mut settings = Settings::default();
    // Listed in the file for whoever edits it, and on.
    assert!(
        serde_json::to_string(&settings)
            .unwrap()
            .contains(r#""rust-analyzer":{"enabled":true"#)
    );
    assert!(settings.runs_server("rust-analyzer", Some(app)));

    settings.set_server("rust-analyzer", &Scope::Project(app.into()), false);
    assert!(!settings.runs_server("rust-analyzer", Some(app)));
    assert!(settings.runs_server("rust-analyzer", Some(other)));
    // Off in a project twice, it's listed once.
    settings.set_server("rust-analyzer", &Scope::Project(app.into()), false);
    assert_eq!(settings.language_servers["rust-analyzer"].off_in.len(), 1);

    settings.set_server("rust-analyzer", &Scope::Everywhere, false);
    assert!(!settings.runs_server("rust-analyzer", Some(other)));
    assert!(!settings.runs_server("rust-analyzer", None));
    // Turned on in a project while off everywhere, it's on everywhere it wasn't turned off.
    settings.set_server("rust-analyzer", &Scope::Project(other.into()), true);
    assert!(settings.runs_server("rust-analyzer", Some(other)));
    assert!(!settings.runs_server("rust-analyzer", Some(app)));
    settings.set_server("rust-analyzer", &Scope::Project(app.into()), true);
    assert!(settings.runs_server("rust-analyzer", Some(app)));

    // A server the file doesn't name runs.
    let named: Settings = serde_json::from_str(r#"{ "language_servers": {} }"#).unwrap();
    assert!(named.runs_server("rust-analyzer", Some(app)));
}
