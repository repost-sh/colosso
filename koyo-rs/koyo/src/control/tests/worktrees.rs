//! Worktrees as the worktree operations change them, in a repository of the test's own: made
//! as the New Worktree dialog makes them, selected, and deleted once the person confirms,
//! never the main checkout and never with work in them unless forced.

use std::process::Command as Process;

use gpui_kit::TestAppContext;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::json;

use super::{allow, call, controlled, external, op, path_of, read_shell, send, taken, terminal};
use crate::model::PaneKind;
use crate::test_support::{repo, select, write};

/// The local branches of the repository at `root`.
fn branches(root: &std::path::Path) -> Vec<String> {
    let listed = Process::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

#[gpui_kit::test]
fn a_worktree_is_made_listed_and_selected(cx: &mut TestAppContext) {
    let root = repo("control-worktree-create", &[("README.md", "hi\n")]);
    let rig = controlled("control-worktree-create", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let created = call(&agent, "worktree_create", json!({ "name": "Feat x" }), cx).unwrap();
    let path = path_of(&created["worktree"]["path"]);
    assert_eq!(created["worktree"]["branch"], json!("feat-x"));
    assert_eq!(
        path,
        rig.worktrees.join(root.file_name().unwrap()).join("feat-x")
    );
    assert!(path.join("README.md").exists());
    read_shell(&rig, cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        assert_eq!(unit.worktree.path, path, "the new worktree is selected");
        assert_eq!(unit.tabs.len(), 1, "with a chat open on it");
        assert_eq!(unit.tabs[0].first().kind, PaneKind::Chat);
    });
    let listed = call(&agent, "worktree_branches", json!({}), cx).unwrap();
    assert_eq!(listed["defaultBase"], json!("main"));
    assert_eq!(listed["prefix"], json!(""));
    assert_eq!(
        listed["worktreesDir"],
        json!(rig.worktrees.join(root.file_name().unwrap()))
    );
    let names: Vec<_> = (listed["branches"].as_array().unwrap().iter())
        .map(|branch| {
            (
                branch["name"].as_str().unwrap().to_owned(),
                branch["current"] == json!(true),
            )
        })
        .collect();
    assert!(names.contains(&("main".to_owned(), true)), "{names:?}");
    assert!(names.contains(&("feat-x".to_owned(), false)), "{names:?}");
    // By its branch, then by its checkout's path.
    let main = call(&agent, "worktree_select", json!({ "worktree": "main" }), cx).unwrap();
    assert_eq!(main["worktree"]["path"], json!(root));
    assert_eq!(main["worktree"]["selected"], json!(true));
    let back = call(&agent, "worktree_select", json!({ "worktree": path }), cx).unwrap();
    assert_eq!(back["worktree"]["branch"], json!("feat-x"));
    // Made without being selected, the selection stays.
    let quiet = call(
        &agent,
        "worktree_create",
        json!({ "name": "quiet", "select": false }),
        cx,
    )
    .unwrap();
    assert!(path_of(&quiet["worktree"]["path"]).exists());
    let selected = read_shell(&rig, cx, |shell, _| {
        shell.workspace.branch().unwrap().worktree.path.clone()
    });
    assert_eq!(selected, path);
    let refused = call(
        &agent,
        "worktree_create",
        json!({ "name": "x", "base": "nope" }),
        cx,
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
}

#[gpui_kit::test]
fn a_worktree_is_deleted_once_the_person_confirms(cx: &mut TestAppContext) {
    let root = repo("control-worktree-delete", &[("README.md", "hi\n")]);
    let rig = controlled("control-worktree-delete", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let created = call(&agent, "worktree_create", json!({ "name": "doomed" }), cx).unwrap();
    let path = path_of(&created["worktree"]["path"]);
    allow("Deleter", &Scope::ALL, cx);
    let client = external("Deleter");
    let refused = call(&client, "worktree_delete", json!({ "worktree": root }), cx).unwrap_err();
    assert_eq!(
        refused.code,
        ErrorCode::InvalidInput,
        "the main checkout is never deleted"
    );
    assert!(
        !cx.has_pending_prompt(),
        "nobody is asked about a call refused anyway"
    );
    write(&path, "notes.txt", b"unsaved work\n");
    let refused = call(
        &client,
        "worktree_delete",
        json!({ "worktree": "doomed" }),
        cx,
    )
    .unwrap_err();
    assert_eq!(
        refused.code,
        ErrorCode::InvalidInput,
        "work in it isn't lost unless forced"
    );
    assert_eq!(refused.details.unwrap()["changes"], json!(["notes.txt"]));
    assert!(!cx.has_pending_prompt());
    let deleting = send(
        &client,
        None,
        op(
            "worktree_delete",
            json!({ "worktree": "doomed", "force": true }),
        ),
        cx,
    );
    cx.run_until_parked();
    let (question, detail) = cx.pending_prompt().expect("the person is asked");
    assert_eq!(question, "“Deleter” wants to delete the worktree “doomed”");
    assert!(detail.contains("1 changed file"), "{detail}");
    cx.simulate_prompt_answer("Delete");
    cx.run_until_parked();
    let deleted = taken(&deleting).unwrap().unwrap();
    assert_eq!(deleted, json!({ "deleted": path, "branch": "doomed" }));
    assert!(!path.exists());
    assert_eq!(
        branches(&root),
        ["main"],
        "its branch, made with it, went too"
    );
    let listed = call(&agent, "project_list", json!({}), cx).unwrap();
    assert_eq!(
        listed["projects"][0]["worktrees"].as_array().unwrap().len(),
        1
    );
}
