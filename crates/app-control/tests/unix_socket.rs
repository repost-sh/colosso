//! The client against a fake app on a real Unix socket: hello and welcome, calls and their
//! answers, errors, many calls at once answered out of order, a lost connection, events.

#![cfg(unix)]

// Marked as test code, as everything here is, so the lints' leave for tests (to unwrap, to
// panic) reaches the helpers too.
#[cfg(test)]
mod common;

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use colosso_app_control::frame::{ClientFrame, MAX_CLIENT_FRAME, MAX_SERVER_FRAME, ServerFrame};
use colosso_app_control::ops::app::{AppStatus, AppStatusOutput};
use colosso_app_control::ops::shared::NoInput;
use colosso_app_control::transport::Listener;
use colosso_app_control::{
    Client, ClientInfo, ControlError, ErrorCode, Hello, Notice, Scope, Topic,
};
use common::{next_call, spawn_app, temp_dir};
use serde_json::json;

/// How long a test waits for something that should come at once, before failing.
const PATIENCE: Duration = Duration::from_secs(10);

fn hello() -> Hello {
    Hello::new(ClientInfo {
        name: "test".into(),
        version: "1.0".into(),
    })
}

#[test]
fn a_hello_is_welcomed_with_the_grant() {
    let (said, heard) = mpsc::channel();
    let (endpoint, app) = spawn_app(
        "hello",
        Scope::DEFAULT_GRANT.to_vec(),
        move |hello, _, _| {
            said.send(hello).unwrap();
        },
    );
    let mut hello = hello();
    hello.terminal = Some("term_01".into());
    hello.worktree = Some("/w/feat".into());
    let client = Client::connect(&endpoint, &hello).unwrap();
    assert_eq!(client.grant(), Scope::DEFAULT_GRANT);
    assert!(client.allows(Scope::Browser));
    assert!(!client.allows(Scope::Destructive));
    let heard = heard.recv_timeout(PATIENCE).unwrap();
    assert_eq!(heard, hello);
    assert_eq!(heard.protocol, 1);
    assert_eq!(heard.pid, std::process::id());
    app.join().unwrap();
}

#[test]
fn a_refused_hello_fails_with_the_apps_error() {
    let endpoint = colosso_app_control::Endpoint::at(temp_dir("refuse").join("app.sock"));
    let listener = Listener::bind(&endpoint).unwrap();
    let app = std::thread::spawn(move || {
        let (mut reader, writer) = listener.accept().unwrap().split().unwrap();
        let _: Option<ClientFrame> = reader.read(MAX_CLIENT_FRAME).unwrap();
        let refusal = ServerFrame::refusal(ControlError::control_disabled());
        writer.write(&refusal, MAX_SERVER_FRAME).unwrap();
    });
    let Err(error) = Client::connect(&endpoint, &hello()) else {
        panic!("the hello was accepted");
    };
    assert_eq!(error, ControlError::control_disabled());
    app.join().unwrap();
}

#[test]
fn a_call_gets_its_output_and_a_typed_call_its_type() {
    let status = json!({
        "version": "0.1.0", "windowOpen": true, "focused": false, "route": "workspace",
        "layout": "orca", "sidebarOpen": true, "rightPanel": null, "bottomPanelOpen": false,
        "selected": { "project": "/p" }, "mcp": { "http": false }, "dataDir": "/d",
    });
    let answer = status.clone();
    let (endpoint, app) = spawn_app("call", vec![Scope::Read], move |_, mut reader, writer| {
        for _ in 0..2 {
            let call = next_call(&mut reader);
            assert_eq!(call.operation, "app_status");
            assert_eq!(call.input, json!({}));
            assert_eq!(call.request_id, None);
            let output = ServerFrame::answer(call.id, Ok(answer.clone()));
            writer.write(&output, MAX_SERVER_FRAME).unwrap();
        }
    });
    let client = Client::connect(&endpoint, &hello()).unwrap();
    assert_eq!(client.call("app_status", json!({}), None).unwrap(), status);
    let typed: AppStatusOutput = client
        .call_op::<AppStatus>(NoInput::default(), None)
        .unwrap();
    assert_eq!(typed.route, "workspace");
    assert_eq!(typed.selected.project.as_deref(), Some("/p"));
    app.join().unwrap();
}

#[test]
fn an_error_comes_back_as_the_app_sent_it_and_the_request_id_goes_along() {
    let (endpoint, app) = spawn_app("error", vec![Scope::Browser], |_, mut reader, writer| {
        let call = next_call(&mut reader);
        assert_eq!(call.operation, "browser_click");
        assert_eq!(call.request_id.as_deref(), Some("click-1"));
        let refused = ServerFrame::answer(call.id, Err(ControlError::person_has_control()));
        writer.write(&refused, MAX_SERVER_FRAME).unwrap();
    });
    let client = Client::connect(&endpoint, &hello()).unwrap();
    let error = client
        .call("browser_click", json!({ "ref": "e3" }), Some("click-1"))
        .unwrap_err();
    assert_eq!(error, ControlError::person_has_control());
    assert!(!error.retryable);
    let malformed = client
        .call("browser_click", json!({}), Some("not ok!"))
        .unwrap_err();
    assert_eq!(malformed.code, ErrorCode::InvalidInput);
    app.join().unwrap();
}

#[test]
fn twenty_calls_from_twenty_threads_are_answered_out_of_order() {
    let (endpoint, app) = spawn_app("many", vec![Scope::Read], |_, mut reader, writer| {
        // Every call is in flight before the first answer goes out, last call first.
        let calls: Vec<_> = (0..20).map(|_| next_call(&mut reader)).collect();
        for call in calls.into_iter().rev() {
            let echo = json!({ "n": call.input["n"].clone() });
            writer
                .write(&ServerFrame::answer(call.id, Ok(echo)), MAX_SERVER_FRAME)
                .unwrap();
        }
    });
    let client = Arc::new(Client::connect(&endpoint, &hello()).unwrap());
    std::thread::scope(|threads| {
        for n in 0..20 {
            let client = Arc::clone(&client);
            threads.spawn(move || {
                let output = client.call("app_status", json!({ "n": n }), None).unwrap();
                assert_eq!(output, json!({ "n": n }));
            });
        }
    });
    app.join().unwrap();
}

#[test]
fn a_lost_connection_fails_waiting_and_later_calls_as_retryable() {
    let (endpoint, app) = spawn_app("lost", vec![Scope::Read], |_, mut reader, writer| {
        let _ = next_call(&mut reader);
        writer.shutdown();
    });
    let client = Client::connect(&endpoint, &hello()).unwrap();
    let notices = client.notices();
    let error = client.call("project_list", json!({}), None).unwrap_err();
    assert_eq!(error.code, ErrorCode::AppNotRunning);
    assert!(error.retryable);
    assert!(client.is_closed());
    assert_eq!(
        notices.recv_timeout(PATIENCE).unwrap(),
        Notice::Lost(error.clone())
    );
    assert_eq!(
        client.call("project_list", json!({}), None).unwrap_err(),
        error
    );
    assert_eq!(client.subscribe(&[Topic::App]).unwrap_err(), error);
    app.join().unwrap();
}

#[test]
fn a_call_left_unanswered_times_out() {
    let (done, finished) = mpsc::channel::<()>();
    let (endpoint, app) = spawn_app("slow", vec![Scope::Read], move |_, mut reader, _| {
        let _ = next_call(&mut reader);
        // Holds the connection open, unanswered, until the test is done with it.
        let _ = finished.recv_timeout(PATIENCE);
    });
    let client = Client::connect(&endpoint, &hello()).unwrap();
    let error = client
        .call_within("app_status", json!({}), None, Duration::from_millis(200))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(error.retryable);
    assert!(!client.is_closed());
    done.send(()).unwrap();
    app.join().unwrap();
}

#[test]
fn a_subscription_delivers_the_apps_events() {
    let (endpoint, app) = spawn_app("events", vec![Scope::Read], |_, mut reader, writer| {
        let subscribed: Option<ClientFrame> = reader.read(MAX_CLIENT_FRAME).unwrap();
        assert_eq!(
            subscribed,
            Some(ClientFrame::Subscribe {
                topics: vec![Topic::App, Topic::Workspace]
            })
        );
        for topic in [Topic::Workspace, Topic::App] {
            writer
                .write(&ServerFrame::Event { topic }, MAX_SERVER_FRAME)
                .unwrap();
        }
        // An answer still finds its call between events.
        let call = next_call(&mut reader);
        writer
            .write(
                &ServerFrame::answer(call.id, Ok(json!({}))),
                MAX_SERVER_FRAME,
            )
            .unwrap();
    });
    let client = Client::connect(&endpoint, &hello()).unwrap();
    let notices = client.notices();
    client.subscribe(&[Topic::App, Topic::Workspace]).unwrap();
    assert_eq!(
        notices.recv_timeout(PATIENCE).unwrap(),
        Notice::Changed(Topic::Workspace)
    );
    assert_eq!(
        notices.recv_timeout(PATIENCE).unwrap(),
        Notice::Changed(Topic::App)
    );
    assert_eq!(
        client.call("settings_get", json!({}), None).unwrap(),
        json!({})
    );
    app.join().unwrap();
}

#[test]
fn nothing_listening_is_app_not_running() {
    let endpoint = colosso_app_control::Endpoint::at(temp_dir("none").join("app.sock"));
    let Err(error) = Client::connect(&endpoint, &hello()) else {
        panic!("connected to nothing");
    };
    assert_eq!(error, ControlError::app_not_running());
    assert!(error.retryable);
}
