//! Projects as the project operations change them: added by a folder in their repository,
//! selected, reordered and taken out, the sidebar's list saved each time.

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::json;

use super::{call, controlled, read_shell, terminal};
use crate::test_support::repo;

#[gpui_kit::test]
fn projects_are_added_selected_moved_and_removed(cx: &mut TestAppContext) {
    let first = repo("control-projects-a", &[("README.md", "a\n")]);
    let second = repo("control-projects-b", &[("src/lib.rs", "pub fn b() {}\n")]);
    let rig = controlled("control-projects", &[&first], cx);
    let agent = terminal("term_1");
    // A folder inside the repository adds it by its main checkout, selected.
    let added = call(
        &agent,
        "project_add",
        json!({ "path": second.join("src") }),
        cx,
    )
    .unwrap();
    assert_eq!(added["project"]["path"], json!(second));
    assert_eq!(added["project"]["selected"], json!(true));
    assert_eq!(added["project"]["worktrees"][0]["branch"], json!("main"));
    assert_eq!(added["project"]["worktrees"][0]["main"], json!(true));
    // By its name, when no other project has it.
    let name = first.file_name().unwrap().to_string_lossy().into_owned();
    let selected = call(&agent, "project_select", json!({ "project": name }), cx).unwrap();
    assert_eq!(selected["project"]["path"], json!(first));
    let order = call(
        &agent,
        "project_move",
        json!({ "project": second, "to": 0 }),
        cx,
    )
    .unwrap();
    assert_eq!(order, json!({ "projects": [second, first] }));
    assert_eq!(
        read_shell(&rig, cx, |shell, _| shell
            .workspace
            .project()
            .map(|project| project.path.clone())),
        Some(first.clone()),
        "the selected project stays selected"
    );
    let removed = call(&agent, "project_remove", json!({ "project": first }), cx).unwrap();
    assert_eq!(removed, json!({ "removed": first }));
    let listed = call(&agent, "project_list", json!({}), cx).unwrap();
    assert_eq!(listed["projects"].as_array().unwrap().len(), 1);
    assert_eq!(listed["projects"][0]["path"], json!(second));
    assert_eq!(
        cx.read(|cx| crate::settings::get(cx).projects.clone()),
        vec![second]
    );
    // The files stay where they are.
    assert!(first.join("README.md").exists());
}

#[gpui_kit::test]
fn a_project_that_isnt_there_is_refused(cx: &mut TestAppContext) {
    let root = repo("control-projects-missing", &[("README.md", "a\n")]);
    let _rig = controlled("control-projects-missing", &[&root], cx);
    let agent = terminal("term_1");
    let refused = call(
        &agent,
        "project_select",
        json!({ "project": "nowhere" }),
        cx,
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
    assert_eq!(refused.details, Some(json!({ "candidates": [root] })));
    let refused = call(
        &agent,
        "project_add",
        json!({ "path": root.join("missing") }),
        cx,
    )
    .unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
}
