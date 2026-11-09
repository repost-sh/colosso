use super::*;
use crate::frame::{read_frame, write_frame};

fn round_trip<T>(message: &T) -> T
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let mut stream = Vec::new();
    write_frame(&mut stream, message).unwrap();
    read_frame(&mut stream.as_slice()).unwrap().unwrap()
}

#[test]
fn every_message_to_the_host_round_trips() {
    let at = Point { x: 10.5, y: 1049. };
    let messages = [
        ToHost::Hello {
            protocol: PROTOCOL,
            version: "0.1.0".into(),
        },
        ToHost::AddArea {
            id: 7,
            slot: 3,
            owner: "terminal:abc /w".into(),
        },
        ToHost::RemoveArea { id: 7 },
        ToHost::Visible {
            id: 7,
            visible: false,
        },
        ToHost::WantKeyFrame { id: 7 },
        ToHost::Input {
            id: 7,
            input: Input::Down {
                at,
                button: Button::Right,
                clicks: 2,
            },
        },
        ToHost::Input {
            id: 7,
            input: Input::Scroll {
                at,
                dx: -1.,
                dy: 12.5,
                phase: 2,
                momentum: 0,
            },
        },
        ToHost::Input {
            id: 7,
            input: Input::Key {
                down: true,
                code: 0,
                flags: 0x10_0000,
                text: Some("a".into()),
            },
        },
        ToHost::Input {
            id: 7,
            input: Input::Pinch {
                at,
                magnification: 0.1,
                phase: 1,
            },
        },
    ];
    for message in messages {
        assert_eq!(round_trip(&message), message);
    }
}

#[test]
fn every_request_to_the_host_round_trips() {
    let at = Point { x: 10.5, y: 1049. };
    let messages = [
        ToHost::Request {
            request: 1,
            area: 7,
            ask: Ask::Act(Act::Drag {
                from: at,
                to: Point { x: 0., y: 0. },
            }),
        },
        ToHost::Request {
            request: 2,
            area: 7,
            ask: Ask::Open {
                path: "/System/Applications/TextEdit.app".into(),
                args: vec!["-x".into()],
                env: [("A".to_owned(), "b".to_owned())].into(),
                new_instance: true,
            },
        },
        ToHost::Request {
            request: 3,
            area: 7,
            ask: Ask::Run {
                command: "ls".into(),
                cwd: None,
                timeout_ms: 1000,
            },
        },
        ToHost::Request {
            request: 4,
            area: 7,
            ask: Ask::Snapshot {
                interactive: true,
                app: Some("TextEdit".into()),
            },
        },
        ToHost::Request {
            request: 5,
            area: 7,
            ask: Ask::Wait {
                until: WaitFor::Change,
                region: Some(Rect {
                    x: 0.,
                    y: 0.,
                    width: 100.,
                    height: 50.,
                }),
                quiet_ms: 400,
                timeout_ms: 10_000,
            },
        },
        ToHost::Request {
            request: 6,
            area: 7,
            ask: Ask::Act(Act::SetValue {
                element: "e4".into(),
                text: "hello".into(),
                append: false,
            }),
        },
        ToHost::Cancel { request: 3 },
        ToHost::Wallpaper {
            png: ByteBuf::from(vec![0x89, b'P', b'N', b'G']),
        },
    ];
    for message in messages {
        assert_eq!(round_trip(&message), message);
    }
}

#[test]
fn every_message_from_the_host_round_trips() {
    let messages = [
        FromHost::Hello {
            protocol: PROTOCOL,
            version: "0.1.0".into(),
            accessibility: true,
            screen_recording: false,
        },
        FromHost::Screen {
            size: Size {
                width: 3360,
                height: 1050,
            },
        },
        FromHost::Areas {
            areas: vec![KeptArea {
                id: 1,
                slot: 0,
                owner: "chat:x".into(),
            }],
        },
        FromHost::Video {
            id: 1,
            key: true,
            pts: 123_456,
            parameter_sets: vec![ByteBuf::from(vec![0x67, 1]), ByteBuf::from(vec![0x68, 2])],
            data: ByteBuf::from(vec![0, 0, 0, 1, 0x65]),
        },
        FromHost::Pointer {
            id: 1,
            at: Point { x: 1., y: 2. },
            pressed: true,
            click: false,
        },
        FromHost::Answer {
            request: 9,
            result: Ok(Answer::Opened {
                app: "TextEdit".into(),
                pid: 42,
                windows: 1,
            }),
        },
        FromHost::Answer {
            request: 11,
            result: Ok(Answer::Waited {
                met: true,
                millis: 420,
                changed: None,
            }),
        },
        FromHost::Answer {
            request: 10,
            result: Err("keyboard focus is outside".into()),
        },
        FromHost::Output {
            request: 3,
            data: ByteBuf::from(b"hello\n".to_vec()),
        },
        FromHost::Log {
            level: LogLevel::Warn,
            text: "a window was put back".into(),
        },
        FromHost::Stats {
            id: 1,
            stats: Stats {
                streaming: true,
                fps: 59.8,
                ..Stats::default()
            },
        },
    ];
    for message in messages {
        assert_eq!(round_trip(&message), message);
    }
}

#[test]
fn a_key_frame_of_video_costs_its_bytes_and_little_more() {
    let data = vec![0xAB; 50_000];
    let message = FromHost::Video {
        id: 1,
        key: false,
        pts: 1,
        parameter_sets: Vec::new(),
        data: ByteBuf::from(data),
    };
    let mut stream = Vec::new();
    write_frame(&mut stream, &message).unwrap();
    assert!(stream.len() < 50_000 + 64, "{}", stream.len());
}
