use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, TestAppContext, point, px, size};

use super::file::{read, reload};
use super::migration::migrate;
use super::*;
use crate::budget::{settle, wakes_a_minute};
use crate::gradient::Harmony;
use crate::test_support::{Blank, in_front, in_window, scratch};

/// Gives a test's settings a file in `folder` to be saved to, as the app's are: its path.
pub(crate) fn saved_in(folder: &Path, cx: &mut App) -> PathBuf {
    let file = folder.join(FILE);
    cx.global_mut::<Store>().path = Some(file.clone());
    file
}

/// Counts what the settings' observers hear, for as long as the subscription is kept.
pub(crate) fn hearing(cx: &mut TestAppContext) -> (Rc<Cell<usize>>, Subscription) {
    let heard = Rc::new(Cell::new(0));
    let counting = heard.clone();
    let observing = cx.update(|cx| observe_app(cx, move |_| counting.set(counting.get() + 1)));
    (heard, observing)
}

/// Starts the settings with a file to be saved to in the scratch folder `name`: its path.
fn started_with_file(name: &str, cx: &mut TestAppContext) -> PathBuf {
    cx.update(init);
    cx.update(|cx| saved_in(&scratch(name), cx))
}

/// Writes `json` to the file at `path` as an edit made outside the app does, dated `secs` after
/// the epoch, so no two edits share a date.
fn edit(path: &Path, json: &str, secs: u64) {
    std::fs::write(path, json).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

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
    assert!(settings.preview_files);
    assert!(settings.inlay_hints);
    assert_eq!(settings.most_language_servers, None);
    assert!(!settings.stop_idle_language_servers);
    assert!(settings.runs_server("rust-analyzer", None));
    assert_eq!(settings.chat_activity, ChatActivity::Subtle);
    assert_eq!(
        settings.markdown_opens_as,
        crate::files::MarkdownMode::Preview
    );
    let limited: Settings = serde_json::from_str(r#"{ "most_language_servers": 2 }"#).unwrap();
    assert_eq!(limited.most_language_servers, Some(2));
}

#[gpui_kit::test]
fn edits_to_the_file_are_taken_up_while_the_app_runs(cx: &mut TestAppContext) {
    let path = started_with_file("settings-reload", cx);
    let delay = |cx: &mut TestAppContext| cx.read(|cx| get(cx).key_repeat.delay_ms);
    edit(&path, r#"{ "key_repeat": { "delay_ms": 90 } }"#, 1);
    cx.update(reload);
    assert_eq!(delay(cx), 90);
    edit(&path, r#"{ "key_repeat": { "delay_ms": 4"#, 2);
    cx.update(reload);
    assert_eq!(delay(cx), 90);
    cx.update(|cx| update(cx, |settings| settings.key_repeat.delay_ms = 120));
    edit(&path, r#"{ "key_repeat": { "delay_ms": 60 } }"#, 3);
    cx.update(reload);
    assert_eq!(delay(cx), 120);
}

/// What the settings' observers hear: a change made in the app, once, and not its save a moment
/// later, nor the file read again as the app wrote it; nor the file touched outside with every
/// setting as it was. An edit outside that changes a setting is taken up, and heard once.
#[gpui_kit::test]
fn observers_hear_of_a_change_once_and_not_of_its_save(cx: &mut TestAppContext) {
    let path = started_with_file("settings-heard", cx);
    let (heard, _observing) = hearing(cx);
    let delay = |cx: &mut TestAppContext| cx.read(|cx| get(cx).key_repeat.delay_ms);

    cx.update(|cx| update(cx, |settings| settings.key_repeat.delay_ms = 120));
    assert_eq!(heard.get(), 1, "the change");
    settle(cx);
    assert_eq!(read(&path).unwrap().key_repeat.delay_ms, 120, "saved");
    assert_eq!(heard.get(), 1, "the save");
    cx.update(reload);
    assert_eq!(heard.get(), 1, "the app's own save read again");

    let saved = std::fs::read_to_string(&path).unwrap();
    edit(&path, &saved, 1);
    cx.update(reload);
    assert_eq!(heard.get(), 1, "the file touched, every setting as it was");
    edit(&path, r#"{ "key_repeat": { "delay_ms": 60 } }"#, 2);
    cx.update(reload);
    assert_eq!(delay(cx), 60, "an edit outside is taken up");
    assert_eq!(heard.get(), 2, "and heard once");
}

/// The file is looked at for an edit made outside the app only while Koyo is in front, every two
/// seconds, so one made there is taken up within them; one made while another app is in front is
/// taken up as Koyo comes back. With no file, nothing looks.
#[gpui_kit::test]
fn the_file_is_looked_at_only_while_koyo_is_in_front(cx: &mut TestAppContext) {
    cx.update(init);
    let (window, _) = in_window(size(px(400.), px(300.)), cx, |_, cx| cx.new(|_| Blank));
    let delay = |cx: &mut TestAppContext| cx.read(|cx| get(cx).key_repeat.delay_ms);
    let to_front = |front: bool, cx: &mut TestAppContext| {
        in_front(window, front, cx);
        if front {
            cx.update(came_to_front);
        }
    };
    to_front(true, cx);
    assert_eq!(wakes_a_minute(cx), 0, "no file to look at");

    let path = cx.update(|cx| saved_in(&scratch("settings-in-front"), cx));
    to_front(true, cx);
    edit(&path, r#"{ "key_repeat": { "delay_ms": 90 } }"#, 1);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(delay(cx), 90, "an edit made in front, taken up within two seconds");
    assert_eq!(wakes_a_minute(cx), 30, "looked at every two seconds in front");

    to_front(false, cx);
    edit(&path, r#"{ "key_repeat": { "delay_ms": 60 } }"#, 2);
    assert_eq!(wakes_a_minute(cx), 0, "not looked at behind");
    assert_eq!(delay(cx), 90, "so an edit made behind waits");
    to_front(true, cx);
    assert_eq!(delay(cx), 60, "an edit made behind, taken up as Koyo comes in front");
}

/// A change still settling is written as the app quits, so it's not lost.
#[gpui_kit::test]
fn a_change_still_settling_is_saved_as_the_app_quits(cx: &mut TestAppContext) {
    let path = started_with_file("settings-quit-due", cx);
    cx.update(|cx| update(cx, |settings| settings.key_repeat.delay_ms = 120));
    cx.quit();
    assert_eq!(read(&path).unwrap().key_repeat.delay_ms, 120);
}

/// A save made already is not written again as the app quits: the file stays as it was left,
/// whoever edited it since.
#[gpui_kit::test]
fn a_save_made_is_not_written_again_as_the_app_quits(cx: &mut TestAppContext) {
    let path = started_with_file("settings-quit-saved", cx);
    cx.update(|cx| update(cx, |settings| settings.key_repeat.delay_ms = 120));
    settle(cx);
    std::fs::write(&path, "left as it was").unwrap();
    cx.quit();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "left as it was");
}

#[test]
fn a_file_with_retired_keys_reads_without_them() {
    let path = scratch("settings-migrate").join(FILE);
    let old = r#"{
        "layout": "Arc",
        "files_width": 280,
        "native_pages": true,
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
    assert!(value.get("native_pages").is_none());
    assert_eq!(value["key_repeat"], serde_json::json!({ "delay_ms": 200 }));
    let settings = read(&path).unwrap();
    assert_eq!(settings.layout, Layout::Arc);
    let repeat = KeyRepeat {
        delay_ms: 200,
        ..KeyRepeat::default()
    };
    assert_eq!(settings.key_repeat, repeat);
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
    assert!(
        serde_json::to_string(&settings)
            .unwrap()
            .contains(r#""rust-analyzer":{"enabled":true"#)
    );
    assert!(settings.runs_server("rust-analyzer", Some(app)));

    settings.set_server("rust-analyzer", &Scope::Project(app.into()), false);
    assert!(!settings.runs_server("rust-analyzer", Some(app)));
    assert!(settings.runs_server("rust-analyzer", Some(other)));
    settings.set_server("rust-analyzer", &Scope::Project(app.into()), false);
    assert_eq!(settings.language_servers["rust-analyzer"].off_in.len(), 1);

    settings.set_server("rust-analyzer", &Scope::Everywhere, false);
    assert!(!settings.runs_server("rust-analyzer", Some(other)));
    assert!(!settings.runs_server("rust-analyzer", None));
    settings.set_server("rust-analyzer", &Scope::Project(other.into()), true);
    assert!(settings.runs_server("rust-analyzer", Some(other)));
    assert!(!settings.runs_server("rust-analyzer", Some(app)));
    settings.set_server("rust-analyzer", &Scope::Project(app.into()), true);
    assert!(settings.runs_server("rust-analyzer", Some(app)));

    let named: Settings = serde_json::from_str(r#"{ "language_servers": {} }"#).unwrap();
    assert!(named.runs_server("rust-analyzer", Some(app)));
}
