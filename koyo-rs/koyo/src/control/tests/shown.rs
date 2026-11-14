//! Whether what an agent makes or opens is shown: as its call's `select` says, else as the
//! person set Switch to what agents open in Settings → MCP server, off until they turn it on.
//! Not shown, it's listed and the window stays where it was: the checkout, the tab that
//! checkout shows, the settings page and the keyboard.

use std::path::PathBuf;

use gpui_kit::TestAppContext;
use serde_json::{Value, json};

use super::super::policy;
use super::{Rig, call, controlled, path_of, read_shell, terminal};
use crate::test_support::{draw, repo, select};

/// Has the person turn Switch to what agents open on or off.
pub(super) fn show_what_agents_open(on: bool, cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::settings::update(cx, |settings| {
            settings.external_control.show_what_agents_open = on;
        });
    });
}

/// The checkout the window shows, and the first pane of the tab it shows.
pub(super) fn shown(rig: &Rig, cx: &TestAppContext) -> (PathBuf, Option<u64>) {
    read_shell(rig, cx, |shell, _| {
        let unit = shell.workspace.branch().expect("a checkout is selected");
        let tab = unit.tabs.get(unit.active_tab);
        (unit.worktree.path.clone(), tab.map(|tab| tab.first().id))
    })
}

/// `input` with `select` as given, or without it.
fn selecting(mut input: Value, select: Option<bool>) -> Value {
    if let Some(select) = select {
        input["select"] = json!(select);
    }
    input
}

/// A worktree `agent` makes called `name`, with `select` as given: its checkout.
fn made(name: &str, select: Option<bool>, cx: &mut TestAppContext) -> PathBuf {
    let input = selecting(json!({ "name": name }), select);
    let created = call(&terminal("term_1"), "worktree_create", input, cx).unwrap();
    path_of(&created["worktree"]["path"])
}

#[gpui_kit::test]
fn a_select_left_out_is_the_persons_setting_and_a_given_one_wins(cx: &mut TestAppContext) {
    let root = repo("control-shown-setting", &[("README.md", "hi\n")]);
    let _rig = controlled("control-shown-setting", &[&root], cx);
    assert!(
        !cx.read(|cx| policy::shows(None, cx)),
        "off until the person turns it on"
    );
    // chat_create shows its tab exactly as this says; a chat needs a host to start, QA's.
    for on in [false, true] {
        show_what_agents_open(on, cx);
        let shows = |select| cx.read(|cx| policy::shows(select, cx));
        assert_eq!(shows(None), on, "left out, the setting: {on}");
        assert!(shows(Some(true)), "asked to show, the setting {on}");
        assert!(!shows(Some(false)), "asked not to, the setting {on}");
    }
}

#[gpui_kit::test]
fn a_worktree_an_agent_makes_is_switched_to_only_as_the_person_or_the_call_says(
    cx: &mut TestAppContext,
) {
    let root = repo("control-shown-worktrees", &[("README.md", "hi\n")]);
    let rig = controlled("control-shown-worktrees", &[&root], cx);
    select(&rig.shell, &root, cx);
    let listed = made("listed", None, cx);
    assert!(listed.join("README.md").exists());
    assert_eq!(shown(&rig, cx).0, root, "off, the window stays");
    let projects = call(&terminal("term_1"), "project_list", json!({}), cx).unwrap();
    let worktrees = projects["projects"][0]["worktrees"].as_array().unwrap();
    assert!(
        (worktrees.iter()).any(|worktree| worktree["path"] == json!(listed)),
        "but the sidebar lists it: {worktrees:?}"
    );
    let asked = made("asked", Some(true), cx);
    assert_eq!(shown(&rig, cx).0, asked, "asked to, it switches");

    show_what_agents_open(true, cx);
    select(&rig.shell, &root, cx);
    let followed = made("followed", None, cx);
    assert_eq!(shown(&rig, cx).0, followed, "on, it switches");
    select(&rig.shell, &root, cx);
    let quiet = made("quiet", Some(false), cx);
    assert!(quiet.join("README.md").exists());
    assert_eq!(shown(&rig, cx).0, root, "asked not to, it stays");
}

#[gpui_kit::test]
fn a_tab_an_agent_opens_leaves_the_window_where_it_was_unless_shown(cx: &mut TestAppContext) {
    let files = [("README.md", "hi\n"), ("src/lib.rs", "pub fn a() {}\n")];
    let root = repo("control-shown-tabs", &files);
    let rig = controlled("control-shown-tabs", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let other = made("other", None, cx);
    let mine = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    let before = (root.clone(), mine["pane"].as_u64());
    assert_eq!(shown(&rig, cx), before, "the person's tab");

    for kind in [
        json!({ "kind": "chat" }),
        json!({ "kind": "editor", "file": "README.md" }),
        // Filling its pane, so it opens at the size a pane assumes undrawn; an agent's page
        // starts at 1440 × 900 otherwise (`viewport.rs`).
        json!({ "kind": "browser", "url": "https://example.com/", "viewport": { "fill": true } }),
        json!({ "kind": "changes" }),
    ] {
        let mut input = kind.clone();
        input["worktree"] = json!(other);
        let opened = call(&agent, "tab_open", input, cx).unwrap();
        assert_eq!(opened["worktree"], json!(other), "{kind}");
        assert_eq!(shown(&rig, cx), before, "{kind} opens out of sight");
    }
    let here = json!({ "kind": "editor", "file": "src/lib.rs" });
    let here = call(&agent, "tab_open", here, cx).unwrap();
    assert_eq!(here["worktree"], json!(root));
    assert_eq!(
        shown(&rig, cx),
        before,
        "on the person's checkout, their tab stays"
    );
    // A page opened out of sight is sized to open in the engine, which only drawing does
    // otherwise, so the browser tools reach it.
    let pages = call(&agent, "browser_list", json!({}), cx).unwrap();
    let page = &pages["pages"][0];
    assert_eq!(page["worktree"], json!(other));
    assert_eq!(page["viewport"]["width"], json!(1280));
    assert_eq!(page["viewport"]["height"], json!(800));

    let asked = json!({ "worktree": other, "kind": "chat", "select": true });
    let asked = call(&agent, "tab_open", asked, cx).unwrap();
    assert_eq!(
        shown(&rig, cx),
        (other.clone(), asked["pane"].as_u64()),
        "asked to"
    );

    show_what_agents_open(true, cx);
    select(&rig.shell, &root, cx);
    assert_eq!(shown(&rig, cx), before);
    let followed = json!({ "worktree": other, "kind": "chat" });
    let followed = call(&agent, "tab_open", followed, cx).unwrap();
    assert_eq!(
        shown(&rig, cx),
        (other.clone(), followed["pane"].as_u64()),
        "on"
    );
    select(&rig.shell, &root, cx);
    let quiet = json!({ "worktree": other, "kind": "chat", "select": false });
    call(&agent, "tab_open", quiet, cx).unwrap();
    assert_eq!(shown(&rig, cx), before, "asked not to");
}

#[gpui_kit::test]
fn a_settings_page_and_the_keyboard_stay_with_the_person(cx: &mut TestAppContext) {
    let files = [("README.md", "hi\n"), ("src/lib.rs", "pub fn a() {}\n")];
    let root = repo("control-shown-keyboard", &files);
    let rig = controlled("control-shown-keyboard", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let status = |cx: &mut TestAppContext| call(&agent, "app_status", json!({}), cx).unwrap();
    let readme = json!({ "path": "README.md", "select": true });
    let readme = call(&agent, "file_open", readme, cx).unwrap()["pane"].clone();
    draw(rig.window, cx);
    let focused = |status: Value| status["selected"]["pane"].clone();
    assert_eq!(
        focused(status(cx)),
        readme,
        "shown, its editor has the keyboard"
    );

    call(&agent, "file_open", json!({ "path": "src/lib.rs" }), cx).unwrap();
    draw(rig.window, cx);
    assert_eq!(
        focused(status(cx)),
        readme,
        "out of sight, it takes no keyboard"
    );
    assert_eq!(shown(&rig, cx), (root.clone(), readme.as_u64()));
    let lib = json!({ "path": "src/lib.rs", "select": true });
    let lib = call(&agent, "file_open", lib, cx).unwrap()["pane"].clone();
    draw(rig.window, cx);
    assert_eq!(
        focused(status(cx)),
        lib,
        "asked to, it's shown with the keyboard"
    );

    call(&agent, "settings_open", json!({ "page": "mcp" }), cx).unwrap();
    for (operation, input) in [
        ("tab_open", json!({ "kind": "chat" })),
        ("tab_open", json!({ "kind": "browser" })),
        ("file_open", json!({ "path": "README.md" })),
        ("changes_open", json!({})),
    ] {
        call(&agent, operation, input.clone(), cx).unwrap();
        assert_eq!(
            status(cx)["route"],
            json!("settings:mcp"),
            "{operation} {input}"
        );
    }
    call(&agent, "changes_open", json!({ "select": true }), cx).unwrap();
    let route = status(cx)["route"].clone();
    assert_eq!(route, json!("workspace"), "asked to, it leaves settings");
}

#[gpui_kit::test]
fn files_changes_and_splits_on_another_checkout_follow_select(cx: &mut TestAppContext) {
    let root = repo("control-shown-panes", &[("README.md", "hi\n")]);
    let rig = controlled("control-shown-panes", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let other = made("other", None, cx);
    let mine = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    let before = (root.clone(), mine["pane"].as_u64());

    let theirs = json!({ "worktree": other, "kind": "chat" });
    let theirs = call(&agent, "tab_open", theirs, cx).unwrap()["pane"].clone();
    let file = json!({ "worktree": other, "path": "README.md" });
    assert_eq!(
        call(&agent, "file_open", file, cx).unwrap()["worktree"],
        json!(other)
    );
    assert_eq!(shown(&rig, cx), before, "file_open");
    let changes = call(&agent, "changes_open", json!({ "worktree": other }), cx).unwrap();
    assert_eq!(changes["worktree"], json!(other));
    assert_eq!(shown(&rig, cx), before, "changes_open");
    let split = json!({ "pane": theirs, "edge": "right", "kind": "chat" });
    let split = call(&agent, "pane_split", split, cx).unwrap();
    assert_eq!(split["worktree"], json!(other));
    assert_eq!(shown(&rig, cx), before, "pane_split");

    let asked = json!({ "pane": theirs, "edge": "bottom", "kind": "chat", "select": true });
    call(&agent, "pane_split", asked, cx).unwrap();
    assert_eq!(
        shown(&rig, cx),
        (other.clone(), theirs.as_u64()),
        "asked to"
    );

    show_what_agents_open(true, cx);
    select(&rig.shell, &root, cx);
    let changes = json!({ "worktree": other });
    let changes = call(&agent, "changes_open", changes, cx).unwrap()["pane"].clone();
    assert_eq!(shown(&rig, cx), (other.clone(), changes.as_u64()), "on");
    select(&rig.shell, &root, cx);
    let quiet = json!({ "worktree": other, "path": "README.md", "select": false });
    call(&agent, "file_open", quiet, cx).unwrap();
    assert_eq!(shown(&rig, cx), before, "asked not to");
}

#[gpui_kit::test]
fn a_project_an_agent_adds_is_selected_only_as_the_person_or_the_call_says(
    cx: &mut TestAppContext,
) {
    let first = repo("control-shown-projects-a", &[("README.md", "a\n")]);
    let second = repo("control-shown-projects-b", &[("README.md", "b\n")]);
    let third = repo("control-shown-projects-c", &[("README.md", "c\n")]);
    let rig = controlled("control-shown-projects", &[&first], cx);
    select(&rig.shell, &first, cx);
    let agent = terminal("term_1");
    let add = |path: &PathBuf, select: Option<bool>, cx: &mut TestAppContext| {
        let input = selecting(json!({ "path": path }), select);
        call(&agent, "project_add", input, cx).unwrap()["project"]["selected"].clone()
    };
    let selected = |cx: &TestAppContext| {
        read_shell(&rig, cx, |shell, _| {
            shell
                .workspace
                .project()
                .map(|project| project.path.clone())
        })
    };
    assert_eq!(add(&second, None, cx), json!(false));
    assert_eq!(selected(cx), Some(first.clone()), "off, listed only");
    show_what_agents_open(true, cx);
    assert_eq!(add(&third, Some(false), cx), json!(false));
    assert_eq!(selected(cx), Some(first.clone()), "asked not to");
    assert_eq!(
        add(&third, None, cx),
        json!(true),
        "on, the one there is selected"
    );
    assert_eq!(selected(cx), Some(third));
    let listed = call(&agent, "project_list", json!({}), cx).unwrap();
    assert_eq!(listed["projects"].as_array().unwrap().len(), 3);
}
