//! The link's messages survive their frames, and read as their names say.

use super::*;
use crate::frame::{read_frame, write_frame};

fn through_a_frame<T: Serialize + for<'de> Deserialize<'de>>(message: &T) -> T {
    let mut wire = Vec::new();
    write_frame(&mut wire, message).unwrap();
    read_frame(&mut wire.as_slice()).unwrap().unwrap()
}

fn window() -> Window {
    Window {
        id: 1833,
        pid: 677,
        app: "TextEdit".into(),
        title: "Untitled".into(),
        frame: Rect {
            x: 100.0,
            y: 60.0,
            width: 900.0,
            height: 600.0,
        },
        layer: 0,
    }
}

#[test]
fn every_message_to_the_host_round_trips() {
    let at = Point { x: 120.5, y: 300.0 };
    for message in [
        ToHost::Hello {
            protocol: PROTOCOL,
            surfaces: "dev.koyo.desk.1a2b".into(),
        },
        ToHost::Visible { visible: true },
        ToHost::Input {
            input: Input::Down {
                at,
                button: Button::Right,
                clicks: 2,
                flags: crate::keys::flags::SHIFT,
            },
        },
        ToHost::Input {
            input: Input::Scroll {
                at,
                dx: 0.0,
                dy: -12.5,
                phase: 2,
                momentum: 0,
            },
        },
        ToHost::Input {
            input: Input::Key {
                down: true,
                code: 36,
                flags: 0,
            },
        },
        ToHost::Input {
            input: Input::Text { text: "ß".into() },
        },
        ToHost::Request {
            request: 7,
            ask: Ask::Open {
                app: "TextEdit".into(),
                document: Some("/tmp/a.txt".into()),
                args: vec!["-x".into()],
                theirs: true,
                consent: Some("yes, bring my Arc over".into()),
            },
        },
        ToHost::Request {
            request: 8,
            ask: Ask::Act(Act::Drag {
                from: Spot::Point(at),
                to: Spot::Element("e12".into()),
                window: Some(1833),
            }),
        },
        ToHost::Request {
            request: 10,
            ask: Ask::Act(Act::Type {
                text: "héllo\n".into(),
                into: Some("e3".into()),
            }),
        },
        ToHost::Request {
            request: 11,
            ask: Ask::Snapshot {
                window: None,
                interactive: true,
                menus: false,
            },
        },
        ToHost::OpenForPerson {
            bundle: "com.apple.calculator".into(),
        },
        ToHost::TakeBack { pid: 677 },
        ToHost::Request {
            request: 12,
            ask: Ask::Close(Close::Window(1833)),
        },
        ToHost::Request {
            request: 13,
            ask: Ask::Close(Close::App(677)),
        },
        ToHost::Request {
            request: 9,
            ask: Ask::Screenshot {
                window: Some(1833),
                scale: 1,
            },
        },
        ToHost::Cancel { request: 9 },
        ToHost::Quit,
    ] {
        assert_eq!(through_a_frame(&message), message);
    }
}

/// The asks that change what the person is looking at, with their word and without.
#[test]
fn the_asks_for_the_focus_round_trip() {
    for ask in [
        Ask::TakeFocus {
            window: Some(1833),
            consent: Some("yes, show it".into()),
        },
        Ask::TakeFocus {
            window: None,
            consent: None,
        },
        Ask::ReturnFocus,
    ] {
        let message = ToHost::Request { request: 14, ask };
        assert_eq!(through_a_frame(&message), message);
    }
}

#[test]
fn every_message_from_the_host_round_trips() {
    for message in [
        FromHost::Hello {
            protocol: PROTOCOL,
            pid: 4242,
            accessibility: true,
            screen_recording: false,
        },
        FromHost::Grants {
            accessibility: true,
            screen_recording: true,
        },
        FromHost::Desktop {
            state: DesktopState::Ready {
                size: Size {
                    width: 1800.0,
                    height: 1169.0,
                },
                scale: 2.0,
            },
        },
        FromHost::Desktop {
            state: DesktopState::Missing {
                why: "Accessibility isn't granted".into(),
            },
        },
        FromHost::Windows {
            windows: vec![window()],
            backdrop: Some(window()),
        },
        FromHost::Windows {
            windows: Vec::new(),
            backdrop: None,
        },
        FromHost::Lent {
            apps: vec![Lent {
                pid: 677,
                app: "Arc".into(),
            }],
        },
        FromHost::Wanted {
            app: "Calculator".into(),
            bundle: "com.apple.calculator".into(),
        },
        FromHost::Pointer {
            at: Point { x: 10.0, y: 20.0 },
            click: true,
        },
        FromHost::Answer {
            request: 7,
            result: Ok(Answer::Opened {
                app: "TextEdit".into(),
                pid: 677,
                windows: vec![window()],
                lent: true,
            }),
        },
        FromHost::Answer {
            request: 8,
            result: Err(Refusal::new(
                RefusalCode::AppIsThePersons,
                "Arc is already running as yours",
            )),
        },
        FromHost::Answer {
            request: 9,
            result: Ok(Answer::Screenshot {
                png: ByteBuf::from(vec![0x89, b'P', b'N', b'G']),
                origin: Point { x: 100.0, y: 60.0 },
                size: Size {
                    width: 900.0,
                    height: 600.0,
                },
                scale: 1,
            }),
        },
        FromHost::Answer {
            request: 12,
            result: Ok(Answer::Copied {
                text: "what was selected".into(),
            }),
        },
        FromHost::Log {
            level: LogLevel::Warn,
            text: "a window left the desktop".into(),
        },
    ] {
        assert_eq!(through_a_frame(&message), message);
    }
}

#[test]
fn a_refusal_names_its_code_in_camel_case() {
    let mut wire = Vec::new();
    ciborium::into_writer(
        &Refusal::new(RefusalCode::NoDesktop, "the desktop isn't there"),
        &mut wire,
    )
    .unwrap();
    let text = String::from_utf8_lossy(&wire);
    assert!(text.contains("noDesktop"), "{text}");
}
