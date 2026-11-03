//! The app's tools and resources end to end: a real MCP client lists both catalogs, calls
//! app tools that the server forwards over the control socket to a fake Koyo, and reads
//! the app's resources; without an app, calls fail `app_not_running`.

#![cfg(unix)]

#[cfg(test)]
pub mod common;

use std::sync::Arc;

use koyo_app_control::{APP_LAUNCH, APP_LAUNCH_TITLE, ControlError, catalog, summarize};
use koyo_term_control::catalog::{CATALOG, Scope};
use koyo_term_mcp::{Launcher, Server, socket_backend};
use common::{
    CLIENT, FakeApp, Heard, call, config, connect, endpoint, failure, relay, scratch, texts,
};
use rmcp::model::{ProtocolVersion, ReadResourceRequestParams, ResourceContents};
use serde_json::{Value, json};

fn tab_opened() -> Value {
    json!({ "pane": 7, "tab": 2, "worktree": "/work/koyo" })
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_both_catalogs_with_their_annotations() {
    let dir = scratch("list");
    let endpoint = endpoint(&dir);
    let launcher = Launcher::new(Ok(endpoint.clone()), Arc::new(|| Ok(())));
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;
    let tools = session.peer().list_all_tools().await.unwrap();
    let find = |name: &str| tools.iter().find(|tool| tool.name == name);

    let terminals: Vec<_> = (CATALOG.iter())
        .filter(|operation| Scope::AGENT.contains(&operation.scope))
        .collect();
    for operation in &terminals {
        let tool = find(operation.name).unwrap_or_else(|| panic!("{} listed", operation.name));
        assert_eq!(tool.title.as_deref(), Some(operation.title));
        let hints = tool.annotations.clone().unwrap();
        assert_eq!(hints.read_only_hint, Some(operation.hints.read_only));
    }
    for descriptor in catalog() {
        let tool = find(descriptor.name).unwrap_or_else(|| panic!("{} listed", descriptor.name));
        assert_eq!(tool.title.as_deref(), Some(descriptor.title));
        assert_eq!(tool.description.as_deref(), Some(descriptor.description));
        assert_eq!(*tool.input_schema, descriptor.input_schema);
        assert_eq!(
            tool.output_schema.as_deref(),
            Some(&descriptor.output_schema)
        );
        let hints = tool.annotations.clone().unwrap();
        assert_eq!(hints.title.as_deref(), Some(descriptor.title));
        let said = (
            hints.read_only_hint,
            hints.destructive_hint,
            hints.idempotent_hint,
            hints.open_world_hint,
        );
        let honest = (
            Some(descriptor.hints.read_only),
            Some(descriptor.hints.destructive),
            Some(descriptor.hints.idempotent),
            Some(false),
        );
        assert_eq!(said, honest, "{}", descriptor.name);
    }
    let launch = find(APP_LAUNCH).unwrap();
    assert_eq!(launch.title.as_deref(), Some(APP_LAUNCH_TITLE));
    let hints = launch.annotations.clone().unwrap();
    assert_eq!(
        (
            hints.read_only_hint,
            hints.destructive_hint,
            hints.open_world_hint
        ),
        (Some(false), Some(false), Some(false))
    );
    assert_eq!(tools.len(), terminals.len() + catalog().len() + 1);

    // A server without a launcher, as the app's own HTTP server is, has no app_launch.
    let (session, _) = connect(
        Server::new(config(&dir), &socket_backend(Ok(endpoint)), None),
        ProtocolVersion::V_2025_11_25,
    )
    .await;
    let tools = session.peer().list_all_tools().await.unwrap();
    assert!(tools.iter().all(|tool| tool.name != APP_LAUNCH));
    assert_eq!(tools.len(), terminals.len() + catalog().len());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_call_is_forwarded_with_its_request_id() {
    let dir = scratch("call");
    let endpoint = endpoint(&dir);
    let app = FakeApp::start(&endpoint, Arc::new(|_| Ok(tab_opened())));
    let (session, _) = connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2025_11_25).await;

    let result = call(
        &session,
        "tab_open",
        json!({ "kind": "terminal", "requestId": "open-1" }),
    )
    .await;

    let Heard::Hello(hello) = app.next() else {
        panic!("the relay said hello first");
    };
    assert_eq!(hello.client.name, CLIENT);
    assert_eq!(hello.client.version, "9.9");
    let forwarded = app.next_call();
    assert_eq!(forwarded.operation, "tab_open");
    assert_eq!(forwarded.request_id.as_deref(), Some("open-1"));
    assert_eq!(forwarded.input, json!({ "kind": "terminal" }));

    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(result.structured_content, Some(tab_opened()));
    let texts = texts(&result);
    assert_eq!(texts[0], summarize("tab_open", &tab_opened()).unwrap());
    assert_eq!(
        serde_json::from_str::<Value>(&texts[1]).unwrap(),
        tab_opened()
    );

    // The second call goes over the same connection.
    let again = call(&session, "tab_open", json!({ "kind": "browser" })).await;
    assert_eq!(again.is_error, Some(false));
    assert_eq!(
        app.next(),
        Heard::Call(app_call(2, "tab_open", json!({ "kind": "browser" })))
    );
}

fn app_call(id: u64, operation: &str, input: Value) -> koyo_app_control::frame::Call {
    koyo_app_control::frame::Call {
        id,
        request_id: None,
        operation: operation.to_owned(),
        input,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_error_is_an_is_error_result_with_the_error() {
    let dir = scratch("error");
    let endpoint = endpoint(&dir);
    let _app = FakeApp::start(
        &endpoint,
        Arc::new(|_| Err(ControlError::not_found("There's no pane 9."))),
    );
    let (session, _) = connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2025_11_25).await;

    let result = call(&session, "pane_focus", json!({ "pane": 9 })).await;
    assert_eq!(
        failure(&result),
        json!({ "code": "not_found", "message": "There's no pane 9.", "retryable": false })
    );
    assert_eq!(texts(&result)[0], "There's no pane 9.");

    // Arguments that don't fit the operation are refused before anything is sent.
    let result = call(
        &session,
        "pane_focus",
        json!({ "pane": 9, "colour": "red" }),
    )
    .await;
    assert_eq!(failure(&result)["code"], "invalid_input");
}

#[tokio::test(flavor = "multi_thread")]
async fn without_the_app_calls_fail_app_not_running_and_app_launch_is_listed() {
    let dir = scratch("absent");
    let endpoint = endpoint(&dir);
    let launcher = Launcher::new(Ok(endpoint.clone()), Arc::new(|| Ok(())));
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;

    let result = call(&session, "app_status", json!({})).await;
    let error = failure(&result);
    assert_eq!(error["code"], "app_not_running");
    assert_eq!(error["retryable"], true);
    assert!(
        error["message"].as_str().unwrap().contains(APP_LAUNCH),
        "{error}"
    );
    let tools = session.peer().list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == APP_LAUNCH));

    // Once the app runs, the next call reaches it.
    let app = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let result = call(&session, "settings_close", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(app.next_call().operation, "settings_close");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_flag_names_the_caller() {
    let dir = scratch("named");
    let endpoint = endpoint(&dir);
    let app = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let mut named = config(&dir);
    named.client = Some("night-shift".into());
    let server = Server::new(named, &socket_backend(Ok(endpoint)), None);
    let (session, _) = connect(server, ProtocolVersion::V_2025_11_25).await;

    let result = call(&session, "app_focus", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let Heard::Hello(hello) = app.next() else {
        panic!("the relay said hello first");
    };
    assert_eq!(hello.client.name, "night-shift");
}

#[tokio::test(flavor = "multi_thread")]
async fn app_resources_are_read_through_their_operations() {
    let dir = scratch("read");
    let endpoint = endpoint(&dir);
    let app = FakeApp::start(
        &endpoint,
        Arc::new(|call| Ok(json!({ "answered": call.operation, "input": call.input }))),
    );
    let (session, _) = connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2025_11_25).await;

    let cases = [
        ("koyo://app", "app_status", json!({})),
        ("koyo://projects", "project_list", json!({})),
        ("koyo://tabs", "tab_list", json!({ "all": true })),
        ("koyo://settings", "settings_get", json!({})),
    ];
    for (uri, operation, input) in cases {
        let read = session
            .peer()
            .read_resource(ReadResourceRequestParams::new(uri))
            .await
            .unwrap();
        let [
            ResourceContents::TextResourceContents {
                uri: read_uri,
                mime_type,
                text,
                ..
            },
        ] = &read.contents[..]
        else {
            panic!("one text content for {uri}: {read:?}");
        };
        assert_eq!(read_uri, uri);
        assert_eq!(mime_type.as_deref(), Some("application/json"));
        let answered = json!({ "answered": operation, "input": input });
        assert_eq!(serde_json::from_str::<Value>(text).unwrap(), answered);
        let forwarded = app.next_call();
        assert_eq!(
            (forwarded.operation.as_str(), &forwarded.input),
            (operation, &input)
        );
    }
}
