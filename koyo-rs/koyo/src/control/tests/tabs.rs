//! Tabs as the tab operations change them: opened of each kind, listed, selected, moved,
//! closed (not over unsaved edits unless they're discarded) and reopened; and the window's
//! panels, layout and settings pages.

use gpui_kit::TestAppContext;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::{Value, json};

use super::{allow, call, controlled, external, op, read_shell, send, taken, terminal};
use crate::files::At;
use crate::files::tests::support::{editing, typed};
use crate::test_support::{draw, repo, select};

/// The kinds of the selected worktree's tabs' panes, the shown tab's in brackets.
fn shown_tabs(listed: &Value) -> Vec<String> {
    let tabs = listed["worktrees"][0]["tabs"].as_array().unwrap();
    (tabs.iter())
        .map(|tab| {
            let kinds: Vec<&str> = (tab["panes"].as_array().unwrap().iter())
                .map(|pane| pane["kind"].as_str().unwrap())
                .collect();
            let name = kinds.join("|");
            if tab["selected"] == json!(true) {
                format!("[{name}]")
            } else {
                name
            }
        })
        .collect()
}

#[gpui_kit::test]
fn tabs_open_select_move_close_and_reopen(cx: &mut TestAppContext) {
    let root = repo(
        "control-tabs",
        &[("README.md", "hi\n"), ("src/lib.rs", "pub fn a() {}\n")],
    );
    let rig = controlled("control-tabs", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let chat = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    assert_eq!(chat["tab"], json!(0));
    assert_eq!(chat["worktree"], json!(root));
    let file =
        json!({ "kind": "editor", "file": "src/lib.rs", "line": 1, "column": 5, "select": true });
    let editor = call(&agent, "tab_open", file, cx).unwrap();
    assert_eq!(editor["tab"], json!(1));
    let page = json!({ "kind": "browser", "url": "https://example.com/", "place": "beside", "select": false });
    let browser = call(&agent, "tab_open", page, cx).unwrap();
    assert_eq!(browser["tab"], json!(2), "beside the shown tab");
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    assert_eq!(
        shown_tabs(&listed),
        ["chat", "[editor]", "browser"],
        "opened unselected, it isn't shown"
    );
    let panes = &listed["worktrees"][0]["tabs"];
    assert_eq!(panes[1]["panes"][0]["file"], json!(root.join("src/lib.rs")));
    assert_eq!(panes[1]["panes"][0]["unsaved"], json!(false));
    assert_eq!(panes[2]["panes"][0]["url"], json!("https://example.com/"));
    assert_eq!(panes[0]["layout"], json!({ "pane": chat["pane"] }));
    let shown = call(&agent, "tab_select", json!({ "index": 0 }), cx).unwrap();
    assert_eq!(shown["pane"], chat["pane"]);
    let moved = call(
        &agent,
        "tab_move",
        json!({ "pane": chat["pane"], "to": 9 }),
        cx,
    )
    .unwrap();
    assert_eq!(moved["tab"], json!(2), "past the end is the end");
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    assert_eq!(shown_tabs(&listed), ["editor", "browser", "[chat]"]);
    let closed = call(&agent, "tab_close", json!({ "pane": chat["pane"] }), cx).unwrap();
    assert_eq!(closed, json!({ "closed": true, "discarded": [] }));
    let reopened = call(&agent, "tab_reopen", json!({}), cx).unwrap();
    assert_eq!(
        (reopened["pane"].clone(), reopened["tab"].clone()),
        (chat["pane"].clone(), json!(2))
    );
    let refused = call(&agent, "tab_reopen", json!({}), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound, "nothing closed is left");
    // Every worktree's tabs, with `all`.
    let all = call(&agent, "tab_list", json!({ "all": true }), cx).unwrap();
    assert_eq!(all["worktrees"].as_array().unwrap().len(), 1);
}

#[gpui_kit::test]
fn a_tab_is_refused_what_its_kind_doesnt_take(cx: &mut TestAppContext) {
    let root = repo("control-tabs-refused", &[("README.md", "hi\n")]);
    let rig = controlled("control-tabs-refused", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let refused = call(
        &agent,
        "tab_open",
        json!({ "kind": "chat", "url": "https://example.com" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidInput);
    let refused = call(
        &agent,
        "tab_open",
        json!({ "kind": "editor", "file": "missing.rs" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::NotFound);
    let refused = call(
        &agent,
        "tab_open",
        json!({ "kind": "terminal", "terminal": "term_gone" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::TerminalNotFound);
    // A name the terminal daemon wouldn't take is refused before a tab is made for it.
    let refused = call(
        &agent,
        "tab_open",
        json!({ "kind": "terminal", "name": "Smoke shell" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::InvalidInput);
    // Tests run no terminal daemon, so a new terminal can't be opened.
    let refused = call(
        &agent,
        "tab_open",
        json!({ "kind": "terminal", "name": "build" }),
        cx,
    );
    assert_eq!(refused.unwrap_err().code, ErrorCode::Unsupported);
    let refused = call(&agent, "tab_select", json!({ "index": 3 }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
    assert!(read_shell(&rig, cx, |shell, _| shell
        .workspace
        .branch()
        .unwrap()
        .tabs
        .is_empty()));
}

#[gpui_kit::test]
fn a_tab_with_unsaved_edits_closes_only_discarding_them(cx: &mut TestAppContext) {
    let root = repo("control-tabs-unsaved", &[("README.md", "hi\n")]);
    let rig = controlled("control-tabs-unsaved", &[&root], cx);
    select(&rig.shell, &root, cx);
    let (_, editor, _) = editing(rig.window, &rig.shell, "README.md", At::Line(0), cx);
    typed(rig.window, &editor, "more ", cx);
    let agent = terminal("term_1");
    let pane = call(&agent, "tab_list", json!({}), cx).unwrap()["worktrees"][0]["tabs"][0]["panes"]
        [0]["pane"]
        .clone();
    let refused = call(&agent, "tab_close", json!({ "pane": pane }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::UnsavedChanges);
    let readme = root.join("README.md").display().to_string();
    assert_eq!(refused.details, Some(json!({ "files": [readme] })));
    allow("Claude Code", &Scope::ALL, cx);
    let closing = send(
        &external("Claude Code"),
        None,
        op("tab_close", json!({ "pane": pane, "discard": true })),
        cx,
    );
    cx.run_until_parked();
    let (_, detail) = cx.pending_prompt().expect("the person is asked");
    assert!(detail.contains(&readme), "{detail}");
    cx.simulate_prompt_answer("Close");
    cx.run_until_parked();
    assert_eq!(
        taken(&closing).unwrap().unwrap(),
        json!({ "closed": true, "discarded": [readme] })
    );
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "hi\n",
        "nothing was saved"
    );
}

#[gpui_kit::test]
fn the_window_s_panels_layout_and_settings_pages_follow_the_calls(cx: &mut TestAppContext) {
    let root = repo("control-layout", &[("README.md", "hi\n")]);
    let rig = controlled("control-layout", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let set =
        json!({ "sidebar": false, "bottomPanel": true, "rightPanel": "files", "layout": "arc" });
    let layout = call(&agent, "layout_set", set, cx).unwrap();
    let expected = json!({ "layout": "arc", "sidebarOpen": false, "rightPanel": "files", "bottomPanelOpen": true });
    assert_eq!(layout, expected);
    let status = call(&agent, "app_status", json!({}), cx).unwrap();
    assert_eq!(status["windowOpen"], json!(true));
    assert_eq!(status["layout"], json!("arc"));
    assert_eq!(status["sidebarOpen"], json!(false));
    assert_eq!(status["rightPanel"], json!("files"));
    assert_eq!(status["selected"]["worktree"], json!(root));
    let layout = call(
        &agent,
        "layout_set",
        json!({ "rightPanel": "none", "sidebar": true }),
        cx,
    )
    .unwrap();
    assert_eq!(layout["rightPanel"], Value::Null);
    assert_eq!(layout["sidebarOpen"], json!(true));
    let shown = call(&agent, "settings_open", json!({ "page": "keyboard" }), cx).unwrap();
    assert_eq!(shown["route"], json!("settings:keyboard"));
    let mcp = call(&agent, "settings_open", json!({ "page": "mcp" }), cx).unwrap();
    assert_eq!(mcp["route"], json!("settings:mcp"));
    let closed = call(&agent, "settings_close", json!({}), cx).unwrap();
    assert_eq!(closed["route"], json!("workspace"));
    let pages = call(&agent, "browser_list", json!({}), cx).unwrap();
    assert_eq!(pages["pages"], json!([]), "no browser page is open");
    let refused = call(&agent, "browser_read", json!({}), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound, "there's no page to read");
}

/// A tab brought back at launch and not shown yet: listed with its address and title, its page
/// answering `browser_unavailable` until its tab is shown, which makes it.
#[gpui_kit::test]
fn a_page_brought_back_waits_to_be_shown_for_the_browser_tools(cx: &mut TestAppContext) {
    let root = repo("control-unshown", &[("README.md", "hi\n")]);
    let rig = controlled("control-unshown", &[&root], cx);
    select(&rig.shell, &root, cx);
    let unloaded = crate::model::Unloaded {
        address: "https://example.com/".into(),
        favicon: None,
    };
    let pane = rig.shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(crate::model::PaneKind::Editor);
        // After the tab shown, which stays shown, as the window brings its tabs back.
        let pane = crate::model::Pane::unloaded(unloaded, "Example".into());
        let id = pane.id;
        let unit = shell.workspace.branch_mut().unwrap();
        unit.tabs.push(crate::model::Tab::Single(pane));
        cx.notify();
        id
    });
    draw(rig.window, cx);
    let agent = terminal("term_1");
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    let tabs = &listed["worktrees"][0]["tabs"];
    assert_eq!(tabs[1]["panes"][0]["url"], json!("https://example.com/"));
    let browsers = call(&agent, "browser_list", json!({}), cx).unwrap();
    let first = &browsers["pages"][0];
    assert_eq!(
        (
            first["pane"].clone(),
            first["url"].clone(),
            first["title"].clone()
        ),
        (json!(pane), json!("https://example.com/"), json!("Example"))
    );
    assert_eq!(first["viewport"]["width"], json!(0), "not laid out");
    let read = call(&agent, "browser_read", json!({ "pane": pane }), cx).unwrap_err();
    assert_eq!(read.code, ErrorCode::BrowserUnavailable);
    let unnamed = call(&agent, "browser_read", json!({}), cx).unwrap_err();
    assert_eq!(unnamed.code, ErrorCode::BrowserUnavailable, "the only page");

    call(&agent, "tab_select", json!({ "index": 1 }), cx).unwrap();
    draw(rig.window, cx);
    let made = read_shell(&rig, cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        unit.tabs[1].first().view().is_some()
    });
    assert!(made, "shown, its page is made");
    let browsers = call(&agent, "browser_list", json!({}), cx).unwrap();
    assert_eq!(browsers["pages"][0]["url"], json!("https://example.com/"));
}
