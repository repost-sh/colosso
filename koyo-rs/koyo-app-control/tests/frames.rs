//! Every frame of the socket, as `docs/app-control.md` 6 writes it: the exact JSON each
//! kind goes out as, read back to the same frame; and the line reader's limits.

use std::io::Cursor;

use koyo_app_control::frame::{
    self, Call, ClientFrame, ClientInfo, FrameError, Hello, MAX_CLIENT_FRAME, ServerFrame,
};
use koyo_app_control::{ControlError, ErrorCode, Scope, Topic};
use serde_json::json;

// Marked as test code, as everything here is, so the lints' leave for tests (to unwrap, to
// panic) reaches the helpers too.
#[cfg(test)]
fn client(frame: &ClientFrame, golden: &str) {
    assert_eq!(serde_json::to_string(frame).unwrap(), golden);
    assert_eq!(&serde_json::from_str::<ClientFrame>(golden).unwrap(), frame);
    assert_eq!(&frame::decode_client(golden.as_bytes()).unwrap(), frame);
}

#[cfg(test)]
fn server(frame: &ServerFrame, golden: &str) {
    assert_eq!(serde_json::to_string(frame).unwrap(), golden);
    assert_eq!(&serde_json::from_str::<ServerFrame>(golden).unwrap(), frame);
}

#[test]
fn hello() {
    let hello = Hello {
        protocol: 1,
        client: ClientInfo {
            name: "Claude Code".into(),
            version: "2.1.0".into(),
        },
        terminal: Some("term_01JD".into()),
        chat: None,
        worktree: Some("/w/feat".into()),
        cwd: Some("/w/feat/src".into()),
        pid: 4242,
    };
    client(
        &ClientFrame::Hello(hello.clone()),
        r#"{"type":"hello","protocol":1,"client":{"name":"Claude Code","version":"2.1.0"},"terminal":"term_01JD","worktree":"/w/feat","cwd":"/w/feat/src","pid":4242}"#,
    );
    // Koyo's MCP server in a chat's session names the chat.
    let chat = Hello {
        terminal: None,
        chat: Some("019a5c1e-7d42-7b3a-9c4e-1f2a3b4c5d6e".into()),
        worktree: None,
        ..hello.clone()
    };
    client(
        &ClientFrame::Hello(chat),
        r#"{"type":"hello","protocol":1,"client":{"name":"Claude Code","version":"2.1.0"},"chat":"019a5c1e-7d42-7b3a-9c4e-1f2a3b4c5d6e","cwd":"/w/feat/src","pid":4242}"#,
    );
    let bare = Hello {
        terminal: None,
        worktree: None,
        cwd: None,
        ..hello
    };
    client(
        &ClientFrame::Hello(bare),
        r#"{"type":"hello","protocol":1,"client":{"name":"Claude Code","version":"2.1.0"},"pid":4242}"#,
    );
}

#[test]
fn call() {
    client(
        &ClientFrame::Call(Call {
            id: 7,
            request_id: Some("open-1".into()),
            operation: "tab_open".into(),
            input: json!({ "kind": "browser", "url": "http://localhost:3000" }),
        }),
        r#"{"type":"call","id":7,"requestId":"open-1","operation":"tab_open","input":{"kind":"browser","url":"http://localhost:3000"}}"#,
    );
    client(
        &ClientFrame::Call(Call {
            id: 8,
            request_id: None,
            operation: "app_status".into(),
            input: json!({}),
        }),
        r#"{"type":"call","id":8,"operation":"app_status","input":{}}"#,
    );
}

#[test]
fn a_call_without_input_has_an_empty_one() {
    let frame = frame::decode_client(br#"{"type":"call","id":3,"operation":"app_status"}"#);
    let Ok(ClientFrame::Call(call)) = frame else {
        panic!("{frame:?}");
    };
    assert_eq!(call.input, json!({}));
}

#[test]
fn subscribe() {
    client(
        &ClientFrame::Subscribe {
            topics: vec![Topic::App, Topic::Workspace, Topic::Settings],
        },
        r#"{"type":"subscribe","topics":["app","workspace","settings"]}"#,
    );
}

#[test]
fn welcome() {
    server(
        &ServerFrame::Welcome {
            grant: Scope::DEFAULT_GRANT.to_vec(),
        },
        r#"{"type":"welcome","grant":["read","navigate","manage","browser"]}"#,
    );
}

#[test]
fn result() {
    server(
        &ServerFrame::answer(7, Ok(json!({ "pane": 12, "tab": 2, "worktree": "/w" }))),
        r#"{"type":"result","id":7,"output":{"pane":12,"tab":2,"worktree":"/w"}}"#,
    );
}

#[test]
fn error_of_a_call() {
    server(
        &ServerFrame::answer(9, Err(ControlError::not_found("No pane 12."))),
        r#"{"type":"error","id":9,"error":{"code":"not_found","message":"No pane 12.","retryable":false}}"#,
    );
    let unsaved = ControlError::unsaved_changes(&["/w/a.rs".into()]);
    let golden = format!(
        r#"{{"type":"error","id":10,"error":{{"code":"unsaved_changes","message":{},"retryable":false,"details":{{"files":["/w/a.rs"]}}}}}}"#,
        serde_json::to_string(&unsaved.message).unwrap()
    );
    server(&ServerFrame::answer(10, Err(unsaved)), &golden);
}

#[test]
fn error_of_a_hello() {
    let declined = ControlError::user_declined("The person denied Claude Code.");
    server(
        &ServerFrame::refusal(declined),
        r#"{"type":"error","error":{"code":"user_declined","message":"The person denied Claude Code.","retryable":false}}"#,
    );
}

#[test]
fn event() {
    server(
        &ServerFrame::Event {
            topic: Topic::Workspace,
        },
        r#"{"type":"event","topic":"workspace"}"#,
    );
}

#[test]
fn a_malformed_call_still_names_its_id() {
    let (id, error) = frame::decode_client(br#"{"type":"call","id":5,"operation":7}"#).unwrap_err();
    assert_eq!(id, Some(5));
    assert_eq!(error.code, ErrorCode::InvalidInput);
    let (id, _) = frame::decode_client(br#"{"type":"subscribe","topics":["nope"]}"#).unwrap_err();
    assert_eq!(id, None);
    let (id, _) = frame::decode_client(b"not json").unwrap_err();
    assert_eq!(id, None);
}

#[test]
fn lines_end_at_newlines_within_their_limit() {
    let mut reader = Cursor::new(b"{\"a\":1}\r\n{\"b\":2}\n".to_vec());
    assert_eq!(
        frame::read_line(&mut reader, 64).unwrap().unwrap(),
        b"{\"a\":1}"
    );
    assert_eq!(
        frame::read_line(&mut reader, 64).unwrap().unwrap(),
        b"{\"b\":2}"
    );
    assert!(frame::read_line(&mut reader, 64).unwrap().is_none());

    let mut long = Cursor::new(vec![b'x'; 100]);
    assert!(matches!(
        frame::read_line(&mut long, 10),
        Err(FrameError::TooLarge(10))
    ));
    let mut cut = Cursor::new(b"{\"half".to_vec());
    assert!(matches!(
        frame::read_line(&mut cut, 64),
        Err(FrameError::Io(_))
    ));
}

#[test]
fn a_frame_over_its_limit_is_never_written() {
    let mut out = Vec::new();
    let frame = ServerFrame::answer(1, Ok(json!("x".repeat(64))));
    assert!(matches!(
        frame::write_frame(&mut out, &frame, 32),
        Err(FrameError::TooLarge(32))
    ));
    assert!(out.is_empty());
    frame::write_frame(&mut out, &frame, MAX_CLIENT_FRAME).unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    assert!(!out[..out.len() - 1].contains(&b'\n'));
}
