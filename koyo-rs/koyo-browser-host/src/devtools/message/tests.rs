use super::{Answered, Event, answer, event};

/// An answer in the page's own session is told by its id, whether it's a result or an error,
/// and its object, as Chromium writes them.
#[test]
fn an_answer_is_told_by_its_id_and_object() {
    assert_eq!(
        answer(r#"{"id":4,"result":{"data":"iVBORw0"}}"#),
        Some((4, Answered::Result, r#"{"data":"iVBORw0"}"#))
    );
    assert_eq!(
        answer(r#"{"id":3,"result":{}}"#),
        Some((3, Answered::Result, "{}"))
    );
    assert_eq!(
        answer(r#"{"id":5,"error":{"code":-32000,"message":"Not supported"}}"#),
        Some((
            5,
            Answered::Error,
            r#"{"code":-32000,"message":"Not supported"}"#
        ))
    );
}

/// An event is told by its method and its parameters, which it may lack.
#[test]
fn an_event_is_told_by_its_method_and_parameters() {
    assert_eq!(
        event(r#"{"method":"Target.targetDestroyed","params":{"targetId":"1234"}}"#),
        Some(Event {
            method: "Target.targetDestroyed",
            params: Some(r#"{"targetId":"1234"}"#),
            session: None,
        })
    );
    assert_eq!(
        event(r#"{"method":"Page.frameResized"}"#),
        Some(Event {
            method: "Page.frameResized",
            params: None,
            session: None,
        })
    );
}

/// A message of a frame's session attached under the page's is told as one of the page's own
/// is, with the session it ends in.
#[test]
fn a_message_of_a_frames_session_is_told_with_its_session() {
    assert_eq!(
        answer(r#"{"id":4,"result":{"data":"x"},"sessionId":"AB12"}"#),
        Some((4, Answered::Result, r#"{"data":"x"}"#))
    );
    assert_eq!(
        event(r#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"AB12"}"#),
        Some(Event {
            method: "Page.loadEventFired",
            params: Some(r#"{"timestamp":1}"#),
            session: Some("AB12"),
        })
    );
    assert_eq!(
        event(r#"{"method":"Page.frameResized","sessionId":"AB12"}"#),
        Some(Event {
            method: "Page.frameResized",
            params: None,
            session: Some("AB12"),
        })
    );
    assert_eq!(
        event(r#"{"method":"Runtime.consoleAPICalled","params":{"text":"a,\"sessionId\":\"Z"}}"#)
            .map(|event| event.session),
        Some(None),
        "a session in what the page said is no session"
    );
}

/// A message written another way is neither, and is read in full.
#[test]
fn a_message_of_another_shape_is_neither() {
    for text in [
        r#"{"result":{},"id":4}"#,
        r#"{"id":"4","result":{}}"#,
        r#"{"method":"","params":{}}"#,
        r#"{"id":4,"result":[]}"#,
        r#"{"id":4,"result":{},"sessionId":"A B"}"#,
        "not json",
    ] {
        assert_eq!((answer(text), event(text)), (None, None), "{text}");
    }
}
