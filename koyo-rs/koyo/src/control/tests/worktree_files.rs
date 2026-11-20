//! The one folder of the person's the browser tools let an agent hand a page or read a `file:`
//! page from: the worktree of the chat or terminal it names, as Koyo has it, and none for a
//! caller that names neither, whatever folder its hello gives.

use std::path::Path;

use gpui_kit::TestAppContext;
use koyo_app_control::{ControlError, Scope};
use koyo_browser_ipc::{AppRequest, HostEvent, PageState, PaneId};
use koyo_term_control::Status;
use serde_json::{Value, json};

use super::{Rig, allow, call, controlled, external, op, send, taken, terminal};
use crate::browser::FakeHost;
use crate::control::Caller;
use crate::test_support::{draw, repo, scratch, select, terminal_record};

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

/// A page of `agent`'s on `host`: the page's pane, and the host's number for it.
fn page_on(host: &FakeHost, rig: &Rig, agent: &Caller, cx: &mut TestAppContext) -> (u64, PaneId) {
    let opening = json!({ "kind": "browser", "url": "https://a.example/", "select": true });
    let pane = call(agent, "tab_open", opening, cx).unwrap()["pane"].as_u64().unwrap();
    draw(rig.window, cx);
    let opened = (host.asked(cx).into_iter())
        .find_map(|asked| match asked {
            AppRequest::Open { pane, .. } => Some(pane),
            _ => None,
        })
        .expect("the page opens in the host");
    (pane, opened)
}

/// The page went to `url` by itself, as a page may go from one `file:` page to another.
fn went(host: &FakeHost, opened: PaneId, url: &str, cx: &mut TestAppContext) {
    let state = PageState {
        url: url.into(),
        ..PageState::default()
    };
    host.says(HostEvent::Page { pane: opened, state }, cx);
}

/// What `agent`'s `operation` on pane `pane` answers, the host answering what it's asked; and
/// whether it asked the host anything.
fn reads(
    host: &FakeHost,
    agent: &Caller,
    (operation, input): (&str, Value),
    cx: &mut TestAppContext,
) -> (Result<Value, ControlError>, bool) {
    let slot = send(agent, None, op(operation, input), cx);
    let mut asked = false;
    for _ in 0..10 {
        let sent = host.answers(cx, |_, _, _| Ok(json!({ "result": { "type": "string", "value": "read" } })));
        asked |= sent.iter().any(|sent| matches!(sent, AppRequest::Cdp { .. } | AppRequest::Navigate { .. }));
        if let Some(answer) = taken(&slot) {
            return (answer, asked);
        }
    }
    panic!("{operation} wasn't answered");
}

#[gpui_kit::test]
fn an_agent_reads_and_opens_no_file_page_outside_its_worktree(cx: &mut TestAppContext) {
    let root = repo("control-file-pages", &[("index.html", "<p>mine</p>\n")]);
    let outside = scratch("control-file-pages-outside");
    std::fs::write(outside.join("id_ed25519"), "key").unwrap();
    let rig = controlled("control-file-pages", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = agent_in(&rig, &root, cx);
    let host = FakeHost::serve("control-file-pages-host", cx);
    let (pane, opened) = page_on(&host, &rig, &agent, cx);
    let read = || ("browser_read", json!({ "pane": pane }));

    let secret = format!("file://{}", outside.join("id_ed25519").display());
    went(&host, opened, &secret, cx);
    for tool in [read(), ("browser_eval", json!({ "pane": pane, "expression": "1" }))] {
        let (answer, asked) = reads(&host, &agent, tool, cx);
        let refused = answer.unwrap_err();
        assert!(refused.message.contains("outside your worktree"), "{}", refused.message);
        assert!(!asked, "the page is asked nothing");
    }
    let navigate = ("browser_navigate", json!({ "pane": pane, "url": secret, "wait": "none" }));
    let (answer, asked) = reads(&host, &agent, navigate, cx);
    assert!(answer.unwrap_err().message.contains("outside your worktree"));
    assert!(!asked, "the page goes nowhere");

    let mine = format!("file://{}", root.join("index.html").display());
    went(&host, opened, &mine, cx);
    let (answer, asked) = reads(&host, &agent, read(), cx);
    assert!(answer.is_ok() && asked, "a page of its worktree is read: {answer:?}");
    let navigate = ("browser_navigate", json!({ "pane": pane, "url": mine, "wait": "none" }));
    let (answer, asked) = reads(&host, &agent, navigate, cx);
    assert!(answer.is_ok() && asked, "and opened: {answer:?}");

    let says = outsider(&root, cx);
    let (pane, opened) = page_on(&host, &rig, &says, cx);
    went(&host, opened, &mine, cx);
    let (answer, asked) = reads(&host, &says, ("browser_read", json!({ "pane": pane })), cx);
    let refused = answer.unwrap_err();
    assert!(refused.message.contains("You name no Koyo chat or terminal"), "{}", refused.message);
    assert!(!asked, "a caller of no worktree reads no file: page");
    went(&host, opened, "https://a.example/", cx);
    let (answer, _) = reads(&host, &says, ("browser_read", json!({ "pane": pane })), cx);
    assert!(answer.is_ok(), "a web page needs no worktree: {answer:?}");
}
