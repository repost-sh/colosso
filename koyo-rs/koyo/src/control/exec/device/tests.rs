use koyo_app_control::ErrorCode;
use koyo_device_wire::{Answer, Refusal, RefusalCode};

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
