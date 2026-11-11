//! Every message survives its frame, and reads on the wire as its name says.

use super::*;
use crate::ask::{Act, Press, Spot};
use crate::input::{Button, Edge, Finger};
use koyo_desk_wire::{read_frame, write_frame};

fn through_a_frame<T: Serialize + for<'de> Deserialize<'de>>(message: &T) -> T {
    let mut wire = Vec::new();
    write_frame(&mut wire, message).unwrap();
    read_frame(&mut wire.as_slice()).unwrap().unwrap()
}

fn screen() -> Screen {
    Screen {
        size: Size {
            width: 402.0,
            height: 874.0,
        },
        scale: 3.0,
        pixels: PixelSize {
            width: 1206,
            height: 2622,
        },
        orientation: Orientation::Portrait,
    }
}

fn at(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn acts() -> Vec<Act> {
    vec![
        Act::Tap {
            spot: Spot::Point(at(201.0, 437.0)),
            count: 2,
            hold_ms: 0,
            fingers: 1,
        },
        Act::Tap {
            spot: Spot::Ref("e12".into()),
            count: 1,
            hold_ms: 900,
            fingers: 3,
        },
        Act::Swipe {
            from: Spot::Point(at(201.0, 860.0)),
            to: Spot::Point(at(201.0, 400.0)),
            ms: 600,
            fingers: 1,
        },
        Act::Scroll {
            spot: Spot::Ref("e3".into()),
            dx: 0.0,
            dy: -300.0,
        },
        Act::Pinch {
            spot: Spot::Point(at(201.0, 437.0)),
            scale: 2.5,
            degrees: -45.0,
            ms: 500,
        },
        Act::Type {
            text: "Hello, World".into(),
        },
        Act::Key {
            combo: "cmd+space".into(),
        },
        Act::Button {
            button: Press::AppSwitcher,
        },
        Act::Turn {
            to: Orientation::LandscapeLeft,
        },
    ]
}

#[test]
fn every_message_to_the_host_round_trips() {
    let mut messages = vec![
        ToHost::Hello {
            protocol: PROTOCOL,
            token: "a1b2".into(),
            surfaces: "dev.koyo.device.12.1".into(),
        },
        ToHost::Visible { visible: true },
        ToHost::Input {
            input: Input::Touches {
                fingers: vec![
                    Finger {
                        id: 0,
                        at: at(10.0, 860.0),
                        down: true,
                    },
                    Finger {
                        id: 1,
                        at: at(30.0, 860.5),
                        down: false,
                    },
                ],
                edge: Some(Edge::Bottom),
            },
        },
        ToHost::Input {
            input: Input::Button {
                button: Button::Home,
                down: true,
            },
        },
        ToHost::Input {
            input: Input::Key {
                usage: 0xE1,
                down: false,
            },
        },
        ToHost::Input {
            input: Input::Text {
                text: "ça va".into(),
            },
        },
        ToHost::Input {
            input: Input::Turn {
                to: Orientation::PortraitUpsideDown,
            },
        },
        ToHost::Cancel { request: 7 },
        ToHost::Quit,
    ];
    let asks = [
        Ask::Screenshot { scale: 2 },
        Ask::Snapshot { text: false },
        Ask::Wait {
            quiet_ms: 300,
            timeout_ms: 10_000,
        },
        Ask::Apps,
        Ask::Install {
            path: "/tmp/App.app".into(),
        },
        Ask::Launch {
            bundle: "com.apple.mobilesafari".into(),
            arguments: vec!["-x".into()],
        },
        Ask::Terminate {
            bundle: "com.apple.Preferences".into(),
        },
        Ask::OpenUrl {
            url: "https://example.com".into(),
        },
    ];
    for (request, ask) in (1..).zip(asks.into_iter().chain(acts().into_iter().map(Ask::Act))) {
        messages.push(ToHost::Request { request, ask });
    }
    for message in messages {
        assert_eq!(through_a_frame(&message), message);
    }
}

#[test]
fn every_message_from_the_host_round_trips() {
    let mut messages = vec![
        FromHost::Hello {
            protocol: PROTOCOL,
            pid: 4242,
        },
        FromHost::Surfaces {
            generation: 2,
            pixels: PixelSize {
                width: 2622,
                height: 1206,
            },
        },
        FromHost::Frame {
            generation: 2,
            slot: 1,
        },
        FromHost::Touched {
            at: vec![at(1.0, 2.0), at(3.0, 4.0)],
            kind: Mark::Move,
        },
        FromHost::Log {
            level: LogLevel::Warn,
            text: "dtuhidd: no answer".into(),
        },
    ];
    for state in [
        State::Starting {
            step: Step::Booting { first: true },
        },
        State::Starting { step: Step::Input },
        State::Ready { screen: screen() },
        State::Locked { screen: screen() },
        State::Gone {
            why: "the simulator shut down".into(),
        },
        State::Unavailable {
            code: Missing::DeveloperMode,
            why: "Developer Mode is off".into(),
        },
    ] {
        messages.push(FromHost::State { state });
    }
    let answers = [
        Ok(Answer::Done),
        Ok(Answer::Acted {
            change: Change::Small,
            region: Some(Rect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0,
            }),
            still: true,
            millis: 412,
        }),
        Ok(Answer::Acted {
            change: Change::None,
            region: None,
            still: true,
            millis: 300,
        }),
        Ok(Answer::Screenshot {
            png: ByteBuf::from(vec![0x89, b'P', b'N', b'G', 0, 255]),
            size: screen().size,
            scale: 1,
        }),
        Ok(Answer::Snapshot {
            tree: "Button \"General\" [ref=e1] @201,300".into(),
            source: Source::Accessibility,
            elements: 1,
            truncated: false,
        }),
        Ok(Answer::Waited {
            still: false,
            millis: 10_000,
        }),
        Ok(Answer::Apps {
            apps: vec![App {
                name: "Settings".into(),
                bundle: "com.apple.Preferences".into(),
            }],
        }),
        Err(Refusal::new(
            RefusalCode::PersonActing,
            "the person touched it",
        )),
        Err(Refusal::new(
            RefusalCode::Unsupported,
            "a real device isn't turned",
        )),
    ];
    for (request, result) in (1..).zip(answers) {
        messages.push(FromHost::Answer { request, result });
    }
    for message in messages {
        assert_eq!(through_a_frame(&message), message);
    }
}

#[test]
fn names_on_the_wire_are_camel_case() {
    let mut wire = Vec::new();
    write_frame(
        &mut wire,
        &ToHost::Request {
            request: 1,
            ask: Ask::Act(Act::Button {
                button: Press::NotificationCenter,
            }),
        },
    )
    .unwrap();
    let text = String::from_utf8_lossy(&wire);
    for name in ["request", "act", "button", "notificationCenter"] {
        assert!(text.contains(name), "{name} in {text:?}");
    }
}

#[test]
fn orientations_turn_by_quarters_and_back() {
    for o in [
        Orientation::Portrait,
        Orientation::LandscapeLeft,
        Orientation::PortraitUpsideDown,
        Orientation::LandscapeRight,
    ] {
        assert_eq!(Orientation::from_quarters(o.quarters()), o);
        assert_eq!(o.sideways(), o.quarters() % 2 == 1);
    }
    assert_eq!(Edge::Bottom.flag(), 3);
}
