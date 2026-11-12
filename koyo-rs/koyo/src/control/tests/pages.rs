//! What the browser tools tell an agent about a page it can't use: whether the engine has it
//! open and on screen, and, for a page that won't open, a refusal at once rather than a wait
//! that runs out. The tests' engine never starts, as a failed one doesn't.

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::json;

use super::{call, controlled, terminal};
use crate::test_support::{repo, select};

#[gpui_kit::test]
fn a_page_the_engine_cant_open_is_listed_so_and_refused_at_once(cx: &mut TestAppContext) {
    let root = repo("control-unopened", &[("README.md", "hi\n")]);
    let rig = controlled("control-unopened", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opening = json!({ "kind": "browser", "url": "https://example.com/" });
    let pane = call(&agent, "tab_open", opening, cx).unwrap()["pane"].clone();

    let listed = call(&agent, "browser_list", json!({}), cx).unwrap();
    let info = &listed["pages"][0];
    assert_eq!(info["pane"], pane);
    assert_eq!(
        (info["open"].clone(), info["onScreen"].clone()),
        (json!(false), json!(false))
    );

    // Waiting for it, or moving it, says why at once, rather than when the time runs out.
    let wait = json!({ "pane": pane, "for": "selector", "value": "#go" });
    let waited = call(&agent, "browser_wait", wait, cx).unwrap_err();
    assert_eq!(waited.code, ErrorCode::BrowserUnavailable);
    assert!(!waited.retryable, "{}", waited.message);
    let went = call(
        &agent,
        "browser_go",
        json!({ "pane": pane, "action": "reload" }),
        cx,
    );
    let went = went.unwrap_err();
    assert_eq!(
        (went.code, went.retryable),
        (ErrorCode::BrowserUnavailable, false)
    );

    // Nothing asked, there's nothing to answer.
    let answer = json!({ "pane": pane, "action": "accept" });
    let answered = call(&agent, "browser_dialog", answer, cx).unwrap_err();
    assert_eq!(answered.code, ErrorCode::NotFound, "{}", answered.message);
}

#[gpui_kit::test]
fn the_app_says_whether_its_window_is_on_screen(cx: &mut TestAppContext) {
    let root = repo("control-on-screen", &[("README.md", "hi\n")]);
    let rig = controlled("control-on-screen", &[&root], cx);
    select(&rig.shell, &root, cx);
    let status = call(&terminal("term_1"), "app_status", json!({}), cx).unwrap();
    assert_eq!(status["onScreen"], json!(true), "a test's window is drawn");
}
