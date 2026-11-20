//! What the browser tools tell an agent about a page it can't use: whether the engine has it
//! open and on screen, and, for a page that won't open, a refusal at once rather than a wait
//! that runs out. The tests' engine never starts, as a failed one doesn't.

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::json;

use super::{call, controlled, read_shell, terminal};
use crate::control::Caller;
use crate::browser::BrowserEvent;
use crate::test_support::{draw, repo, select};

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

#[gpui_kit::test]
fn a_tab_the_person_asked_to_go_to_is_shown_and_a_link_opened_beside_is_not(cx: &mut TestAppContext) {
    let root = repo("control-go-to-tab", &[("README.md", "hi\n")]);
    let rig = controlled("control-go-to-tab", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opening = json!({ "kind": "browser", "url": "https://example.com/", "select": true });
    let page = call(&agent, "tab_open", opening, cx).unwrap()["pane"].as_u64().unwrap();
    let view = read_shell(&rig, cx, |shell, _| shell.workspace.pane(page)?.view().cloned()).expect("the page");
    let shown = |cx: &mut TestAppContext| {
        draw(rig.window, cx);
        call(&agent, "app_status", json!({}), cx).unwrap()["selected"]["pane"].clone()
    };
    assert_eq!(shown(cx), json!(page));

    view.update(cx, |_, cx| cx.emit(BrowserEvent::OpenTab("https://b.example/".into())));
    cx.run_until_parked();
    assert_eq!(shown(cx), json!(page), "a link's tab opens behind");

    view.update(cx, |_, cx| cx.emit(BrowserEvent::GoToTab("chrome://extensions/".into())));
    cx.run_until_parked();
    let now = shown(cx);
    assert_ne!(now, json!(page), "the tab asked for is shown");
    let listed = call(&agent, "browser_list", json!({}), cx).unwrap();
    let pages = listed["pages"].as_array().expect("pages");
    let there = pages.iter().find(|info| info["pane"] == now).expect("listed");
    assert_eq!(there["url"], json!("chrome://extensions/"));
    assert_eq!(pages.len(), 3);
}

#[gpui_kit::test]
fn an_agent_gives_files_only_from_the_worktree_of_a_chat_or_terminal_koyo_knows(cx: &mut TestAppContext) {
    let root = repo("control-upload-root", &[("README.md", "hi\n")]);
    let rig = controlled("control-upload-root", &[&root], cx);
    select(&rig.shell, &root, cx);
    let says = Caller {
        worktree: Some(root.clone()),
        cwd: Some(root.clone()),
        ..terminal("term_nobody")
    };
    let opening = json!({ "kind": "browser", "url": "https://example.com/" });
    let pane = call(&says, "tab_open", opening, cx).unwrap()["pane"].clone();
    let upload = json!({ "pane": pane, "paths": ["README.md"] });
    let refused = call(&says, "browser_upload", upload, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound, "{}", refused.message);
    assert!(
        refused.message.contains("no worktree is known as yours"),
        "the worktree its hello names is its own word: {}",
        refused.message
    );
}
