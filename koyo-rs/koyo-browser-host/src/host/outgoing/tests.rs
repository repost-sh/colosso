use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use koyo_browser_ipc::{self as ipc, HostEvent};
use serde_json::json;

use super::{Next, Outgoing, write_all};
use crate::blocking;
use crate::devtools::message::Answered;

/// What the app reads once `handed` has been written to its socket.
fn read_by_the_app(handed: Vec<Outgoing>) -> Vec<HostEvent> {
    let (mut app, host) = UnixStream::pair().unwrap();
    let (sender, written) = mpsc::channel();
    for outgoing in handed {
        sender.send(Next::Write(outgoing)).unwrap();
    }
    drop(sender);
    let writer =
        std::thread::spawn(move || blocking::in_test(|may| write_all(may, host, &written)));
    let mut read = Vec::new();
    while let Some(event) = ipc::read_frame::<HostEvent>(&mut app).unwrap() {
        read.push(event);
    }
    writer.join().unwrap();
    read
}

/// An agent's answer from a page reaches the app as the reply to its call: the result, or the
/// error's message.
#[test]
fn an_answer_reaches_the_app_as_the_reply_to_its_call() {
    let read = read_by_the_app(vec![
        Outgoing::Answer {
            pane: 1,
            call: 7,
            answered: Answered::Result,
            json: r#"{"data":"iVBORw0"}"#.into(),
        },
        Outgoing::Answer {
            pane: 1,
            call: 8,
            answered: Answered::Error,
            json: r#"{"code":-32000,"message":"Not supported"}"#.into(),
        },
    ]);
    assert_eq!(
        read,
        [
            HostEvent::CdpReply {
                pane: 1,
                call: 7,
                result: Some(json!({"data": "iVBORw0"})),
                error: None,
            },
            HostEvent::CdpReply {
                pane: 1,
                call: 8,
                result: None,
                error: Some("Not supported".into()),
            },
        ]
    );
}

/// A result too large for a frame is refused to the call it answers, and what follows it still
/// reaches the app.
#[test]
fn a_result_too_large_for_a_frame_is_refused_to_its_call() {
    let screenshot = format!(r#"{{"data":"{}"}}"#, "A".repeat(koyo_frame::MAX_FRAME));
    let read = read_by_the_app(vec![
        Outgoing::Answer {
            pane: 2,
            call: 9,
            answered: Answered::Result,
            json: screenshot,
        },
        Outgoing::Event(HostEvent::Closed { pane: 2 }),
    ]);
    assert_eq!(
        read,
        [
            HostEvent::CdpReply {
                pane: 2,
                call: 9,
                result: None,
                error: Some("The result is too large to send.".into()),
            },
            HostEvent::Closed { pane: 2 },
        ]
    );
}

/// An event an agent hears reaches the app with its parameters, or none, and the session of
/// the frame it's of.
#[test]
fn an_event_for_an_agent_reaches_the_app_with_its_parameters() {
    let read = read_by_the_app(vec![
        Outgoing::AgentEvent {
            pane: 3,
            session: None,
            method: "Page.loadEventFired".into(),
            params: Some(r#"{"timestamp":1.5}"#.into()),
        },
        Outgoing::AgentEvent {
            pane: 3,
            session: Some("AB12".into()),
            method: "Page.frameResized".into(),
            params: None,
        },
    ]);
    assert_eq!(
        read,
        [
            HostEvent::CdpEvent {
                pane: 3,
                session: None,
                method: "Page.loadEventFired".into(),
                params: json!({"timestamp": 1.5}),
            },
            HostEvent::CdpEvent {
                pane: 3,
                session: Some("AB12".into()),
                method: "Page.frameResized".into(),
                params: serde_json::Value::Null,
            },
        ]
    );
}
