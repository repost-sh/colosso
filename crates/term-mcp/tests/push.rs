//! The app's resources followed end to end: a real MCP client subscribes (with
//! `resources/subscribe`, and with `subscriptions/listen`), the fake app pushes its topic,
//! and the client hears `notifications/resources/updated` for the resources that topic
//! covers; after the app restarts, the server follows the topics again by itself.

#![cfg(unix)]

#[cfg(test)]
pub mod common;

use std::sync::Arc;

use colosso_app_control::Topic;
use common::{FakeApp, HEARING, Heard, call, connect, endpoint, legacy, next, relay, scratch};
use rmcp::model::{ProtocolVersion, ServerNotification, SubscriptionFilter};
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn a_subscribed_resource_hears_its_topic() {
    let dir = scratch("push");
    let endpoint = endpoint(&dir);
    let app = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let (session, mut updates) =
        connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2025_11_25).await;

    legacy(&session, "colosso://projects", true).await;
    assert_eq!(app.next_subscribe(), [Topic::Workspace]);

    app.push(Topic::Workspace);
    assert_eq!(next(&mut updates).await, "colosso://projects");

    // A second resource on the same topic needs no second subscribe.
    legacy(&session, "colosso://tabs", true).await;
    app.push(Topic::Workspace);
    let mut heard = vec![next(&mut updates).await, next(&mut updates).await];
    heard.sort();
    assert_eq!(heard, ["colosso://projects", "colosso://tabs"]);

    // Unsubscribed, a resource is no longer told about.
    legacy(&session, "colosso://projects", false).await;
    app.push(Topic::Workspace);
    assert_eq!(next(&mut updates).await, "colosso://tabs");
    let result = call(&session, "app_status", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert!(updates.try_recv().is_err(), "nothing more was said");
}

#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_listen_hears_the_topic() {
    let dir = scratch("listen");
    let endpoint = endpoint(&dir);
    let app = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let (session, _) = connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2026_07_28).await;

    let filter = SubscriptionFilter::builder()
        .resource_subscriptions(["colosso://settings", "colosso://nothing"])
        .build();
    let mut listening = session.peer().listen(filter).await.unwrap();
    assert_eq!(
        listening.acknowledged().resource_subscriptions,
        Some(vec!["colosso://settings".to_owned()]),
        "only resources that exist are accepted"
    );
    assert_eq!(app.next_subscribe(), [Topic::Settings]);

    app.push(Topic::Settings);
    let notification = tokio::time::timeout(HEARING, listening.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let ServerNotification::ResourceUpdatedNotification(updated) = notification else {
        panic!("an update: {notification:?}");
    };
    assert_eq!(updated.params.uri, "colosso://settings");
}

#[tokio::test(flavor = "multi_thread")]
async fn subscriptions_follow_the_app_again_after_it_restarts() {
    let dir = scratch("restart");
    let endpoint = endpoint(&dir);
    let first = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let (session, mut updates) =
        connect(relay(&dir, &endpoint, None), ProtocolVersion::V_2025_11_25).await;
    legacy(&session, "colosso://app", true).await;
    assert_eq!(first.next_subscribe(), [Topic::App]);

    first.stop();
    let second = FakeApp::start(&endpoint, Arc::new(|_| Ok(json!({}))));
    let Heard::Hello(hello) = second.next() else {
        panic!("the relay connected again");
    };
    assert_eq!(hello.client.name, common::CLIENT);
    assert_eq!(second.next_subscribe(), [Topic::App]);
    // What changed while the app was away is unknown, so the resource is said to have changed.
    assert_eq!(next(&mut updates).await, "colosso://app");

    second.push(Topic::App);
    assert_eq!(next(&mut updates).await, "colosso://app");
    let result = call(&session, "app_status", json!({})).await;
    assert_eq!(result.is_error, Some(false), "{result:?}");
    assert_eq!(second.next_call().operation, "app_status");
}
