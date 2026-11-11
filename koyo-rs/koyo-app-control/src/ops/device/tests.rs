use serde_json::json;

use super::*;
use crate::{Command, ControlError, ErrorCode, catalog};

#[test]
fn reads_need_read_and_acts_need_devices() {
    let devices: Vec<_> = (catalog().into_iter())
        .filter(|descriptor| descriptor.name.starts_with("device_"))
        .collect();
    assert_eq!(devices.len(), 19);
    let reads = [
        "device_list",
        "device_screenshot",
        "device_snapshot",
        "device_wait",
        "device_apps",
    ];
    for descriptor in devices {
        let read = reads.contains(&descriptor.name);
        assert_eq!(descriptor.hints.read_only, read, "{}", descriptor.name);
        let scope = if read { Scope::Read } else { Scope::Devices };
        assert_eq!(descriptor.scope, scope, "{}", descriptor.name);
        assert!(!descriptor.confirm, "{}", descriptor.name);
    }
    assert!(!Scope::DEFAULT_GRANT.contains(&Scope::Devices));
    assert!(Scope::AGENT.contains(&Scope::Devices));
    assert_eq!(Scope::from_id("devices"), Some(Scope::Devices));
}

#[test]
fn an_act_lands_on_a_ref_or_a_point_never_both() {
    assert_eq!(spot(Some("e3"), None, None).unwrap(), Spot::Ref("e3".into()));
    assert_eq!(spot(None, Some(10.), Some(20.)).unwrap(), Spot::Point(10., 20.));
    for (reference, x, y) in [
        (Some("e3"), Some(1.), Some(2.)),
        (None, Some(1.), None),
        (None, None, None),
        (None, Some(f64::NAN), Some(1.)),
    ] {
        let error = spot(reference, x, y).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{reference:?} {x:?} {y:?}");
    }
}

#[test]
fn a_screenshot_is_one_to_three_pixels_a_point_and_a_wait_is_bounded() {
    for (scale, ok) in [(None, true), (Some(1), true), (Some(3), true), (Some(0), false), (Some(4), false)] {
        let shot = DeviceScreenshotInput {
            scale,
            ..DeviceScreenshotInput::default()
        };
        assert_eq!(shot.scale().is_ok(), ok, "{scale:?}");
    }
    let wait = DeviceWaitInput::default();
    assert_eq!((wait.quiet_ms(), wait.timeout_ms()), (500, 10_000));
    let long = DeviceWaitInput {
        quiet_ms: Some(60_000),
        timeout_ms: Some(120_000),
        ..DeviceWaitInput::default()
    };
    assert_eq!((long.quiet_ms(), long.timeout_ms()), (10_000, 60_000));
}

#[test]
fn inputs_refuse_fields_they_dont_have() {
    for (name, input) in [
        ("device_tap", json!({ "x": 1, "y": 2, "button": "left" })),
        ("device_open", json!({ "model": "iPhone 17 Pro", "colour": "blue" })),
        ("device_launch", json!({ "bundle": "com.apple.Preferences", "args": [] })),
    ] {
        let error = Command::parse(name, input).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{name}");
    }
    let parsed = Command::parse("device_tap", json!({ "device": "UDID", "ref": "e4" })).unwrap();
    assert!(matches!(parsed, Command::DeviceTap(_)));
}

#[test]
fn a_device_away_is_retryable_and_one_needing_the_person_is_not() {
    let away = ControlError::device_unavailable("iPhone 17 Pro is starting.");
    assert_eq!(away.code.id(), "device_unavailable");
    assert!(away.retryable);
    let person = ControlError::device_needs_person("Unlock the iPad to continue.");
    assert_eq!(person.code.id(), "device_needs_person");
    assert!(!person.retryable);
}
