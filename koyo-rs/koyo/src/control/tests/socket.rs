//! App control's two ways in: a connection on the control socket, its frames read and
//! written on threads of their own, and an in-process client calling from a thread. The test
//! thread answers what they hand the UI thread, as the app's loop would.

use futures::StreamExt as _;
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::future::{Either, select};
use gpui_kit::TestAppContext;
use koyo_app_control::frame::{MAX_CLIENT_FRAME, MAX_SERVER_FRAME};
use koyo_app_control::transport::{Connection, FrameReader};
use koyo_app_control::{ClientFrame, ClientInfo, ErrorCode, Scope, ServerFrame};
use serde_json::{Value, json};

use super::{Rig, allow, controlled, hello_of, op, terminal};
use crate::control::listener::serve;
use crate::control::{Control, InProcess, Request, handle};
use crate::test_support::repo;

/// Answers the next request a connection or client hands the UI thread, and what follows
/// from it; fails once the thread making them has ended, `done` dropped, instead of
/// waiting for a request that won't come.
fn answer_next(rig: &mut Rig, done: &mut oneshot::Receiver<()>, cx: &mut TestAppContext) {
    let request = match block_on(select(rig.requests.next(), done)) {
        Either::Left((Some(request), _)) => request,
        Either::Left((None, _)) => panic!("app control's channel closed"),
        Either::Right(_) => panic!("the thread ended without the request awaited"),
    };
    cx.update(|cx| handle(request, cx));
    cx.run_until_parked();
}

fn next_frame(reader: &mut FrameReader) -> Value {
    let frame: ServerFrame = reader.read(MAX_SERVER_FRAME).unwrap().expect("a frame");
    serde_json::to_value(frame).unwrap()
}

#[gpui_kit::test]
fn a_socket_client_says_hello_and_its_calls_are_answered(cx: &mut TestAppContext) {
    let root = repo("control-socket", &[("README.md", "hi\n")]);
    let mut rig = controlled("control-socket", &[&root], cx);
    cx.update(|cx| Control::get_mut(cx).test_terminals.insert("term_1".into()));
    let requests = cx.read(|cx| Control::get(cx).requests.clone());
    let (app_end, client_end) = Connection::pair().unwrap();
    let (done, mut served) = oneshot::channel::<()>();
    let serving = std::thread::spawn(move || {
        let _done = done;
        serve(app_end, &requests);
    });
    let (mut reader, writer) = client_end.split().unwrap();
    let hello = ClientFrame::Hello(hello_of(&terminal("term_1")));
    writer.write(&hello, MAX_CLIENT_FRAME).unwrap();
    answer_next(&mut rig, &mut served, cx);
    let grant: Vec<&str> = Scope::AGENT.iter().map(|scope| scope.id()).collect();
    assert_eq!(
        next_frame(&mut reader),
        json!({ "type": "welcome", "grant": grant })
    );
    // An operation that doesn't exist is answered without the UI thread.
    let unknown = json!({ "type": "call", "id": 7, "operation": "nope", "input": {} });
    writer.write(&unknown, MAX_CLIENT_FRAME).unwrap();
    let answer = next_frame(&mut reader);
    assert_eq!(
        (answer["id"].clone(), answer["error"]["code"].clone()),
        (json!(7), json!("invalid_input"))
    );
    let bad_id =
        json!({ "type": "call", "id": 8, "requestId": "no spaces", "operation": "tab_reopen" });
    writer.write(&bad_id, MAX_CLIENT_FRAME).unwrap();
    assert_eq!(
        next_frame(&mut reader)["error"]["code"],
        json!("invalid_input")
    );
    let status = json!({ "type": "call", "id": 9, "operation": "app_status", "input": {} });
    writer.write(&status, MAX_CLIENT_FRAME).unwrap();
    answer_next(&mut rig, &mut served, cx);
    let answer = next_frame(&mut reader);
    assert_eq!(answer["type"], json!("result"));
    assert_eq!(answer["id"], json!(9));
    assert_eq!(answer["output"]["windowOpen"], json!(true));
    // A second hello ends the connection.
    writer.write(&hello, MAX_CLIENT_FRAME).unwrap();
    let refusal = next_frame(&mut reader);
    assert_eq!(refusal["type"], json!("error"));
    assert!(refusal.get("id").is_none());
    serving.join().unwrap();
    let closed = block_on(rig.requests.next());
    assert!(
        matches!(closed, Some(Request::Closed { .. })),
        "the connection says it ended"
    );
    assert!(
        reader
            .read::<ServerFrame>(MAX_SERVER_FRAME)
            .unwrap()
            .is_none(),
        "the app hung up"
    );
}

#[gpui_kit::test]
fn a_connection_that_doesnt_say_hello_first_is_refused(cx: &mut TestAppContext) {
    let root = repo("control-socket-rude", &[("README.md", "hi\n")]);
    let _rig = controlled("control-socket-rude", &[&root], cx);
    let requests = cx.read(|cx| Control::get(cx).requests.clone());
    let (app_end, client_end) = Connection::pair().unwrap();
    let serving = std::thread::spawn(move || serve(app_end, &requests));
    let (mut reader, writer) = client_end.split().unwrap();
    let call = json!({ "type": "call", "id": 1, "operation": "app_status" });
    writer.write(&call, MAX_CLIENT_FRAME).unwrap();
    let refusal = next_frame(&mut reader);
    assert_eq!(refusal["error"]["message"], json!("Say hello first."));
    serving.join().unwrap();
}

#[gpui_kit::test]
fn an_in_process_client_goes_through_the_same_gate(cx: &mut TestAppContext) {
    let root = repo("control-in-process", &[("README.md", "hi\n")]);
    let mut rig = controlled("control-in-process", &[&root], cx);
    allow("MCP over HTTP", &Scope::DEFAULT_GRANT, cx);
    let client = cx.read(InProcess::new).expect("app control has started");
    let before = client.call(None, op("app_status", json!({}))).unwrap_err();
    assert_eq!(
        before.code,
        ErrorCode::InvalidInput,
        "a call before the hello"
    );
    let (done, mut called) = oneshot::channel::<()>();
    let calling = std::thread::spawn(move || {
        let _done = done;
        let info = ClientInfo {
            name: "MCP over HTTP".into(),
            version: "test".into(),
        };
        let grant = client.hello(info).unwrap();
        let status = client.call(None, op("app_status", json!({}))).unwrap();
        let refused = client
            .call(None, op("settings_set", json!({ "previewFiles": false })))
            .unwrap_err();
        let (tell, told) = std::sync::mpsc::channel();
        client
            .subscribe(vec![koyo_app_control::Topic::Workspace], move |topic| {
                tell.send(topic).ok();
            })
            .unwrap();
        (grant, status, refused, told, client)
    });
    for _ in 0..4 {
        answer_next(&mut rig, &mut called, cx);
    }
    let (grant, status, refused, _told, client) = calling.join().unwrap();
    assert_eq!(grant, Scope::DEFAULT_GRANT.to_vec());
    assert_eq!(status["windowOpen"], json!(true));
    assert_eq!(refused.code, ErrorCode::ScopeDenied);
    drop(client);
    let closed = block_on(rig.requests.next()).expect("the client says it's gone");
    cx.update(|cx| handle(closed, cx));
    assert_eq!(
        cx.read(|cx| Control::get(cx).hub.followers()),
        0,
        "a client dropped follows nothing"
    );
}
