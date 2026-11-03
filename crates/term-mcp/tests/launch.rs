//! `app_launch` end to end through a real MCP client, with an injected spawner, so no test
//! ever starts the real app: a stopped app is started and waited for, a running one is left
//! alone, and one that never listens times out.

#![cfg(unix)]

#[cfg(test)]
pub mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use koyo_app_control::{APP_LAUNCH, Endpoint};
use koyo_term_mcp::{Launcher, Spawner};
use common::{FakeApp, call, connect, endpoint, failure, relay, scratch, texts};
use parking_lot::Mutex;
use rmcp::model::ProtocolVersion;
use serde_json::json;

/// A fake start: counts the starts and, when `listens`, starts the fake app on `endpoint`
/// as a started Koyo would.
struct Start {
    starts: Arc<AtomicUsize>,
    app: Arc<Mutex<Option<FakeApp>>>,
}

impl Start {
    fn new(endpoint: &Endpoint, listens: bool) -> (Start, Spawner) {
        let start = Start {
            starts: Arc::default(),
            app: Arc::default(),
        };
        let (starts, app, endpoint) = (
            Arc::clone(&start.starts),
            Arc::clone(&start.app),
            endpoint.clone(),
        );
        let spawn: Spawner = Arc::new(move || {
            starts.fetch_add(1, Ordering::SeqCst);
            if listens {
                let started = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
                *app.lock() = Some(started);
            }
            Ok(())
        });
        (start, spawn)
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_app_is_started_and_waited_for() {
    let dir = scratch("launch");
    let endpoint = endpoint(&dir);
    let (start, spawn) = Start::new(&endpoint, true);
    let launcher = Launcher::new(Ok(endpoint.clone()), spawn).wait(Duration::from_secs(10));
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;

    let result = call(&session, APP_LAUNCH, json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let output = result.structured_content.clone().unwrap();
    assert_eq!(
        (&output["launched"], &output["alreadyRunning"]),
        (&json!(true), &json!(false))
    );
    assert!(output["waitedMs"].is_u64(), "{output}");
    assert!(
        texts(&result)[0].starts_with("Started Koyo"),
        "{result:?}"
    );
    assert_eq!(start.starts.load(Ordering::SeqCst), 1);

    // The started app answers the next call.
    let result = call(&session, "app_focus", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let app = start.app.lock().take().unwrap();
    assert_eq!(app.next_call().operation, "app_focus");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_app_is_left_alone() {
    let dir = scratch("running");
    let endpoint = endpoint(&dir);
    let _app = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let (start, spawn) = Start::new(&endpoint, true);
    let launcher = Launcher::new(Ok(endpoint.clone()), spawn);
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;

    let result = call(&session, APP_LAUNCH, json!({})).await;
    assert_eq!(
        result.structured_content,
        Some(json!({ "launched": false, "alreadyRunning": true, "waitedMs": 0 }))
    );
    assert_eq!(start.starts.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_app_that_never_listens_times_out() {
    let dir = scratch("silent");
    let endpoint = endpoint(&dir);
    let (start, spawn) = Start::new(&endpoint, false);
    let launcher = Launcher::new(Ok(endpoint.clone()), spawn).wait(Duration::from_millis(300));
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;

    let result = call(&session, APP_LAUNCH, json!({})).await;
    assert_eq!(failure(&result)["code"], "timeout");
    assert_eq!(start.starts.load(Ordering::SeqCst), 1);

    // A start that fails says why.
    let failing: Spawner = Arc::new(|| Err("there's no such program".into()));
    let launcher = Launcher::new(Ok(endpoint.clone()), failing);
    let (session, _) = connect(
        relay(&dir, &endpoint, Some(launcher)),
        ProtocolVersion::V_2025_11_25,
    )
    .await;
    let error = failure(&call(&session, APP_LAUNCH, json!({})).await);
    assert_eq!(error["code"], "internal");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("there's no such program"),
        "{error}"
    );
}
