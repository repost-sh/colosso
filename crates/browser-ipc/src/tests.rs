use super::*;

#[test]
fn frames_round_trip_and_a_clean_end_is_none() {
    let messages = vec![
        AppRequest::Open {
            pane: 7,
            url: "https://example.com".into(),
            viewport: Viewport {
                width: 800,
                height: 600,
                scale: 2.,
            },
            profile: "project-a".into(),
        },
        AppRequest::Key {
            pane: 7,
            kind: KeyKind::Char,
            key_code: 65,
            native_key_code: 0,
            text: Some("é".into()),
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        },
        AppRequest::Shutdown,
    ];
    let mut buf = Vec::new();
    for m in &messages {
        write_frame(&mut buf, m).expect("write");
    }
    let mut reader = buf.as_slice();
    for m in &messages {
        assert_eq!(
            read_frame::<AppRequest>(&mut reader)
                .expect("read")
                .as_ref(),
            Some(m)
        );
    }
    assert_eq!(read_frame::<AppRequest>(&mut reader).expect("eof"), None);
}

#[test]
fn a_truncated_or_oversized_frame_is_an_error() {
    let mut buf = Vec::new();
    write_frame(
        &mut buf,
        &HostEvent::Cursor {
            pane: 1,
            cursor: Cursor::Text,
        },
    )
    .expect("write");
    buf.truncate(buf.len() - 1);
    assert!(read_frame::<HostEvent>(&mut buf.as_slice()).is_err());
    let huge = u32::try_from(MAX_FRAME + 1).expect("fits").to_le_bytes();
    assert!(read_frame::<HostEvent>(&mut huge.as_slice()).is_err());
}

#[test]
fn the_wire_names_are_stable() {
    let event = HostEvent::Frame {
        pane: 3,
        generation: 2,
        slot: 1,
        size: PixelSize {
            width: 300,
            height: 200,
        },
    };
    assert_eq!(
        serde_json::to_value(&event).expect("json"),
        serde_json::json!({
            "type": "frame", "pane": 3, "generation": 2, "slot": 1,
            "size": {"width": 300, "height": 200}
        })
    );
    let go = AppRequest::Go {
        pane: 1,
        to: Go::Back,
    };
    assert_eq!(
        serde_json::to_value(&go).expect("json"),
        serde_json::json!({"type": "go", "pane": 1, "to": "back"})
    );
    let devtools = AppRequest::OpenDevTools {
        pane: 1,
        devtools: 2,
        viewport: Viewport {
            width: 300,
            height: 200,
            scale: 1.,
        },
        inspect: Some(Point { x: 10., y: 20.5 }),
    };
    assert_eq!(
        serde_json::to_value(&devtools).expect("json"),
        serde_json::json!({
            "type": "openDevTools", "pane": 1, "devtools": 2,
            "viewport": {"width": 300, "height": 200, "scale": 1.0},
            "inspect": {"x": 10.0, "y": 20.5}
        })
    );
    assert_eq!(devtools.pane(), Some(1), "it acts on the page");
    assert_eq!(
        serde_json::to_value(HostEvent::Closed { pane: 2 }).expect("json"),
        serde_json::json!({"type": "closed", "pane": 2})
    );
}

#[test]
fn dialogs_and_answers_have_stable_wire_names() {
    let dialog = HostEvent::Dialog {
        pane: 2,
        dialog: 9,
        origin: "https://example.com".into(),
        question: Dialog::Prompt {
            message: "Name?".into(),
            default_text: "Ada".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(&dialog).expect("json"),
        serde_json::json!({
            "type": "dialog", "pane": 2, "dialog": 9, "origin": "https://example.com",
            "question": {"kind": "prompt", "message": "Name?", "defaultText": "Ada"}
        })
    );
    let answer = AppRequest::Answer {
        pane: 2,
        dialog: 9,
        answer: Answer::Credentials {
            username: "ada".into(),
            password: "secret".into(),
        },
    };
    assert_eq!(
        serde_json::to_value(&answer).expect("json"),
        serde_json::json!({
            "type": "answer", "pane": 2, "dialog": 9,
            "answer": {"kind": "credentials", "username": "ada", "password": "secret"}
        })
    );
    let unknown: Vec<Permission> =
        serde_json::from_value(serde_json::json!(["camera", "somethingNew"])).expect("json");
    assert_eq!(unknown, [Permission::Camera, Permission::Other]);
}

#[test]
fn hello_needs_the_same_protocol_and_token() {
    let hello = |protocol, token: &str| Hello {
        protocol,
        token: token.into(),
    };
    assert!(check_hello(&hello(PROTOCOL, "secret"), "secret").is_ok());
    assert!(check_hello(&hello(PROTOCOL, "secreT"), "secret").is_err());
    assert!(check_hello(&hello(PROTOCOL, "secre"), "secret").is_err());
    assert!(check_hello(&hello(PROTOCOL + 1, "secret"), "secret").is_err());
}

#[test]
fn viewports_round_to_device_pixels() {
    let v = Viewport {
        width: 401,
        height: 300,
        scale: 1.5,
    };
    assert_eq!(
        v.pixels(),
        PixelSize {
            width: 602,
            height: 450
        }
    );
    let broken = Viewport {
        width: 0,
        height: 10,
        scale: f32::NAN,
    };
    assert_eq!(
        broken.pixels(),
        PixelSize {
            width: 1,
            height: 10
        }
    );
}
