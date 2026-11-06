//! Panes as the pane operations change them: split with a new pane or one moved, resized,
//! given the keyboard, taken out into a tab of their own, and closed; and files opened and
//! saved.

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::{Value, json};

use super::{call, controlled, terminal};
use crate::files::At;
use crate::files::tests::support::{editing, typed};
use crate::test_support::{draw, repo, select};

/// The layout of the selected worktree's tab at `index`.
fn layout(listed: &Value, index: usize) -> Value {
    listed["worktrees"][0]["tabs"][index]["layout"].clone()
}

#[gpui_kit::test]
fn panes_split_resize_detach_and_close(cx: &mut TestAppContext) {
    let root = repo("control-panes", &[("README.md", "hi\n")]);
    let rig = controlled("control-panes", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let first = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap()["pane"].clone();
    let split = json!({ "pane": first, "edge": "right", "kind": "chat" });
    let second = call(&agent, "pane_split", split, cx).unwrap();
    assert_eq!(second["tab"], json!(0), "the new pane is in the split tab");
    let second = second["pane"].clone();
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    let row = json!({ "axis": "row", "ratio": 0.5, "first": { "pane": first }, "second": { "pane": second } });
    assert_eq!(layout(&listed, 0), row);
    let resized = call(
        &agent,
        "pane_resize",
        json!({ "pane": second, "ratio": 0.3 }),
        cx,
    )
    .unwrap();
    assert_eq!(
        resized,
        json!({ "pane": second, "ratio": 0.3 }),
        "as written"
    );
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    assert_eq!(
        layout(&listed, 0)["ratio"],
        json!(0.7),
        "the first side takes the rest"
    );
    let clamped = call(
        &agent,
        "pane_resize",
        json!({ "pane": first, "ratio": 0.01 }),
        cx,
    )
    .unwrap();
    assert_eq!(
        clamped["ratio"],
        json!(0.1),
        "the smallest share a side keeps"
    );
    let refused = call(
        &agent,
        "pane_resize",
        json!({ "pane": first, "ratio": 1.5 }),
        cx,
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    // A pane of another tab moved into the split.
    let third = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap()["pane"].clone();
    let moved = call(
        &agent,
        "pane_split",
        json!({ "pane": first, "edge": "bottom", "move": third }),
        cx,
    )
    .unwrap();
    assert_eq!(moved["tab"], json!(0));
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    assert_eq!(
        listed["worktrees"][0]["tabs"].as_array().unwrap().len(),
        1,
        "the moved pane's tab went"
    );
    assert_eq!(layout(&listed, 0)["first"]["axis"], json!("column"));
    let refused = call(
        &agent,
        "pane_split",
        json!({ "pane": first, "edge": "left", "move": first }),
        cx,
    );
    assert_eq!(
        refused.unwrap_err().code,
        ErrorCode::InvalidInput,
        "a pane can't split itself"
    );
    let detached = call(&agent, "pane_detach", json!({ "pane": second }), cx).unwrap();
    assert_eq!(detached["tab"], json!(1), "a tab of its own right after");
    let alone = call(&agent, "pane_detach", json!({ "pane": second }), cx).unwrap();
    assert_eq!(alone["tab"], json!(1), "a pane alone stays where it is");
    let refused = call(
        &agent,
        "pane_resize",
        json!({ "pane": second, "ratio": 0.5 }),
        cx,
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput, "no split holds it");
    let closed = call(&agent, "pane_close", json!({ "pane": third }), cx).unwrap();
    assert_eq!(closed, json!({ "closed": true, "discarded": [] }));
    let listed = call(&agent, "tab_list", json!({}), cx).unwrap();
    assert_eq!(layout(&listed, 0), json!({ "pane": first }));
    let refused = call(&agent, "pane_focus", json!({ "pane": 999_999 }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
}

#[gpui_kit::test]
fn files_open_split_take_the_keyboard_and_save(cx: &mut TestAppContext) {
    let root = repo(
        "control-files",
        &[("README.md", "hi\n"), ("src/lib.rs", "pub fn a() {}\n")],
    );
    let rig = controlled("control-files", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let readme = call(&agent, "file_open", json!({ "path": "README.md" }), cx).unwrap();
    assert_eq!(readme["tab"], json!(0));
    let split = json!({ "path": root.join("src/lib.rs"), "line": 1, "split": "right" });
    let lib = call(&agent, "file_open", split, cx).unwrap();
    assert_eq!(lib["tab"], json!(0), "split beside the file shown");
    let focused = call(&agent, "pane_focus", json!({ "pane": lib["pane"] }), cx).unwrap();
    assert_eq!(focused["pane"], lib["pane"]);
    draw(rig.window, cx);
    let status = call(&agent, "app_status", json!({}), cx).unwrap();
    assert_eq!(
        status["selected"]["pane"], lib["pane"],
        "the pane has the keyboard"
    );
    let none = call(&agent, "file_save", json!({ "all": true }), cx).unwrap();
    assert_eq!(
        none,
        json!({ "saved": [] }),
        "nothing unsaved, nothing written"
    );
    let (_, editor, _) = editing(rig.window, &rig.shell, "README.md", At::Line(0), cx);
    typed(rig.window, &editor, "more ", cx);
    let saved = call(&agent, "file_save", json!({ "pane": readme["pane"] }), cx).unwrap();
    assert_eq!(saved, json!({ "saved": [root.join("README.md")] }));
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "more hi\n"
    );
    let refused = call(&agent, "file_save", json!({}), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    let changes = call(&agent, "changes_open", json!({}), cx).unwrap();
    assert_eq!(changes["tab"], json!(1));
    let again = call(&agent, "changes_open", json!({}), cx).unwrap();
    assert_eq!(
        again["pane"], changes["pane"],
        "one Changes tab per checkout"
    );
}
