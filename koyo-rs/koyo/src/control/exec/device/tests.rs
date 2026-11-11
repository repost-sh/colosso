use koyo_app_control::ErrorCode;
use koyo_device_wire::{Answer, Refusal, RefusalCode};

use super::opening::room_in;
use super::{person_using, refused, unexpected};

#[test]
fn the_hosts_refusals_read_as_app_control_codes() {
    for (code, expected, retryable) in [
        (RefusalCode::NotReady, ErrorCode::DeviceUnavailable, true),
        (RefusalCode::Gone, ErrorCode::DeviceUnavailable, true),
        (RefusalCode::Locked, ErrorCode::DeviceNeedsPerson, false),
        (RefusalCode::NotFound, ErrorCode::NotFound, false),
        (RefusalCode::Invalid, ErrorCode::InvalidInput, false),
        (RefusalCode::Unsupported, ErrorCode::Unsupported, false),
        (RefusalCode::TimedOut, ErrorCode::Timeout, true),
        (RefusalCode::PersonActing, ErrorCode::PersonHasControl, true),
        (RefusalCode::Failed, ErrorCode::Internal, false),
    ] {
        let error = refused(Refusal::new(code, "As the host said it."));
        assert_eq!(error.code, expected, "{code:?}");
        assert_eq!(error.retryable, retryable, "{code:?}");
        assert_eq!(error.message, "As the host said it.", "{code:?}");
    }
    let using = person_using("iPhone 17 Pro");
    assert!(using.retryable && using.message.contains("iPhone 17 Pro"));
    assert_eq!(unexpected(&Answer::Done).code, ErrorCode::Internal);
}

#[test]
fn the_limit_counts_what_the_set_has_booted() {
    let simulator = |udid: &str, booted| crate::devices::Simulator {
        udid: udid.into(),
        name: format!("iPhone {udid}"),
        system: "iOS 27.2".into(),
        booted,
        booted_before: true,
        ipad: false,
    };
    let none = Vec::new();
    let two = [simulator("A", true), simulator("B", true), simulator("C", false)];
    let full = room_in(&two, &none, Some("C")).unwrap_err();
    assert_eq!(full.code, ErrorCode::DeviceUnavailable);
    assert!(full.message.contains("iPhone A, iPhone B") && !full.retryable, "{}", full.message);
    assert!(room_in(&two, &none, Some("A")).is_ok(), "one already running is no more");
    assert!(room_in(&two, &none, None).is_err(), "a new one is one more");
    let one = [simulator("A", true), simulator("C", false)];
    assert!(room_in(&one, &none, Some("C")).is_ok());
    // One booted and one starting that hasn't booted yet: two, counted together.
    let starting = vec![("D".to_owned(), "iPhone D".to_owned())];
    assert!(room_in(&one, &starting, Some("C")).is_err());
    assert!(room_in(&one, &starting, Some("D")).is_ok(), "the one starting is no more");
}
