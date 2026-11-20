//! The one folder of the person's an agent gives a page files from (`browser_upload`): the
//! worktree of the chat or terminal it names, as Koyo has it, and none for a caller that names
//! neither or one Koyo doesn't know, whatever folder its hello gives.

use std::path::Path;

use gpui_kit::TestAppContext;
use koyo_app_control::{ErrorCode, Scope};
use koyo_term_control::Status;
use serde_json::json;

use super::{Rig, allow, call, controlled, external, hello, terminal};
use crate::control::{Caller, Control};
use crate::test_support::{repo, select, terminal_record};

/// An agent in Koyo terminal `term_1`, which runs in the checkout at `root` as the window has it.
fn agent_in(rig: &Rig, root: &Path, cx: &mut TestAppContext) -> Caller {
    let record = terminal_record("term_1", &root.to_string_lossy(), Status::Idle, "");
    rig.shell.update(cx, |shell, _| shell.terminals.heard_of(&record));
    terminal("term_1")
}

/// A client that names no Koyo chat or terminal, allowed every agent's scope, whose hello says
/// it works in `root`.
fn outsider(root: &Path, cx: &mut TestAppContext) -> Caller {
    allow("outsider", &Scope::AGENT, cx);
    Caller {
        worktree: Some(root.to_path_buf()),
        cwd: Some(root.to_path_buf()),
        ..external("outsider")
    }
}

#[gpui_kit::test]
fn a_caller_naming_no_chat_or_terminal_uploads_nothing(cx: &mut TestAppContext) {
    let root = repo("control-unnamed-upload", &[("README.md", "hi\n")]);
    let rig = controlled("control-unnamed-upload", &[&root], cx);
    select(&rig.shell, &root, cx);
    let opening = json!({ "kind": "browser", "url": "https://example.com/" });
    let says = outsider(&root, cx);
    let pane = call(&says, "tab_open", opening.clone(), cx).unwrap()["pane"].clone();
    let refused = call(&says, "browser_upload", json!({ "pane": pane, "paths": ["README.md"] }), cx)
        .unwrap_err();
    assert!(
        refused.message.contains("You name no Koyo chat or terminal"),
        "its hello's folder isn't its worktree: {}",
        refused.message
    );
    let agent = agent_in(&rig, &root, cx);
    let pane = call(&agent, "tab_open", opening, cx).unwrap()["pane"].clone();
    let refused = call(&agent, "browser_upload", json!({ "pane": pane, "paths": ["README.md"] }), cx)
        .unwrap_err();
    assert!(
        refused.message.contains("isn't asking for files"),
        "its terminal's worktree is known, and the page asks for none: {}",
        refused.message
    );
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

#[gpui_kit::test]
fn a_terminal_the_daemon_runs_that_the_window_never_heard_of_gives_files_once_it_says_hello(
    cx: &mut TestAppContext,
) {
    let root = repo("control-upload-daemons", &[("README.md", "hi\n")]);
    let rig = controlled("control-upload-daemons", &[&root], cx);
    select(&rig.shell, &root, cx);
    let record = terminal_record("term_9", &root.to_string_lossy(), Status::Idle, "");
    cx.update(|cx| Control::get_mut(cx).test_terminals.push(record));
    let agent = terminal("term_9");
    hello(&agent, cx).expect("the daemon runs it");
    let opening = json!({ "kind": "browser", "url": "https://example.com/" });
    let pane = call(&agent, "tab_open", opening, cx).unwrap()["pane"].clone();
    let refused = call(&agent, "browser_upload", json!({ "pane": pane, "paths": ["README.md"] }), cx)
        .unwrap_err();
    assert!(
        refused.message.contains("isn't asking for files"),
        "its worktree is known, and the page asks for none: {}",
        refused.message
    );
}
