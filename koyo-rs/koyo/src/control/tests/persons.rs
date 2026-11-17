//! The person's own pages, pinned and loose, as the browser tools see them: listed, but used by
//! no tool without the person's consent, never meant by a call that names no pane, and the
//! consent one client's, on the connection it asked on, for one page, as long as its lease
//! lasts and it may drive pages. The window tools leave them be. The tests' engine never
//! starts, so a call the gate lets through fails `browser_unavailable` after it: what matters
//! is that it isn't `consent_required`.

use gpui_kit::TestAppContext;
use koyo_app_control::{ControlError, ErrorCode, Scope};
use serde_json::{Value, json};

use super::{Rig, allow, call, controlled, external, op, read_shell, send, taken, terminal};
use crate::control::Caller;
use crate::control::exec::DECLINED_FOR;
use crate::test_support::{repo, select};

/// Opens a page at `url` in the selected worktree as `agent` does, then makes it the person's,
/// as their drag would: pinned, or one of the space's loose pages. Its pane.
pub(super) fn persons_page(
    rig: &Rig,
    agent: &Caller,
    url: &str,
    pinned: bool,
    cx: &mut TestAppContext,
) -> u64 {
    let opened = call(agent, "tab_open", json!({ "kind": "browser", "url": url }), cx).unwrap();
    let pane = opened["pane"].as_u64().unwrap();
    rig.shell.update(cx, |shell, cx| {
        if pinned {
            assert!(shell.pin_tab_of(pane, cx));
        } else {
            shell.loosen_tab_of(pane, usize::MAX, cx);
        }
    });
    cx.run_until_parked();
    pane
}

/// `caller` claims the person's page `pane`, and the person allows it: what the prompt asked.
pub(super) fn allowed(caller: &Caller, pane: u64, cx: &mut TestAppContext) -> String {
    let claiming = send(caller, None, op("browser_claim", json!({ "pane": pane })), cx);
    cx.run_until_parked();
    let (question, _) = cx.pending_prompt().expect("the person is asked");
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    let held = taken(&claiming).expect("answered").unwrap();
    assert_eq!(held, json!({ "pane": pane, "held": true }));
    question
}

/// What `caller` reading `pane` fails with.
pub(super) fn reading(caller: &Caller, pane: u64, cx: &mut TestAppContext) -> ControlError {
    call(caller, "browser_read", json!({ "pane": pane }), cx).unwrap_err()
}

/// Pane `pane` as `browser_list` lists it for `caller`.
pub(super) fn listed(caller: &Caller, pane: u64, cx: &mut TestAppContext) -> Value {
    let pages = call(caller, "browser_list", json!({}), cx).unwrap()["pages"].clone();
    let pages = pages.as_array().unwrap().clone();
    let found = pages.into_iter().find(|page| page["pane"] == json!(pane));
    found.expect("listed")
}

/// What a browser tool does with the person's page, unasked.
enum Does {
    /// Answers, as listing and letting a page go do.
    Answers,
    /// Asks the person.
    Asks,
    /// Refuses with `consent_required`.
    Refuses,
}

/// Every browser tool the registry has, so one added later fails here until it's said what it
/// does with the person's page.
#[gpui_kit::test]
fn every_browser_tool_needs_the_persons_consent_for_their_page(cx: &mut TestAppContext) {
    let root = repo("control-persons-tools", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-tools", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = Caller {
        worktree: Some(root.clone()),
        ..terminal("term_1")
    };
    let pane = persons_page(&rig, &agent, "https://mail.example/", true, cx);
    let tools = (koyo_app_control::infos().into_iter())
        .filter(|info| info.name.starts_with("browser_"))
        .map(|info| info.name)
        .collect::<Vec<_>>();
    assert!(tools.len() > 15, "{tools:?}");
    for name in tools {
        let (input, does) = match name {
            "browser_list" => (json!({}), Does::Answers),
            "browser_release" => (json!({ "pane": pane }), Does::Answers),
            "browser_claim" | "browser_duplicate" => (json!({ "pane": pane }), Does::Asks),
            "browser_navigate" => (
                json!({ "pane": pane, "url": "https://example.com/" }),
                Does::Refuses,
            ),
            "browser_go" => (json!({ "pane": pane, "action": "reload" }), Does::Refuses),
            "browser_eval" => (json!({ "pane": pane, "expression": "1" }), Does::Refuses),
            "browser_click" => (json!({ "pane": pane, "x": 1, "y": 1 }), Does::Refuses),
            "browser_type" => (json!({ "pane": pane, "text": "a" }), Does::Refuses),
            "browser_press_key" => (json!({ "pane": pane, "key": "Enter" }), Does::Refuses),
            "browser_scroll" => (json!({ "pane": pane, "dy": 10 }), Does::Refuses),
            "browser_wait" => (json!({ "pane": pane, "for": "load" }), Does::Refuses),
            "browser_viewport" => (
                json!({ "pane": pane, "width": 800, "height": 600 }),
                Does::Refuses,
            ),
            "browser_dialog" => (json!({ "pane": pane, "action": "accept" }), Does::Refuses),
            "browser_read" | "browser_snapshot" | "browser_screenshot" => {
                (json!({ "pane": pane }), Does::Refuses)
            }
            other => panic!("{other} is a browser tool this test doesn't know: say what it does with the person's page"),
        };
        match does {
            Does::Answers => {
                let answered = call(&agent, name, input, cx);
                assert!(answered.is_ok(), "{name}: {answered:?}");
            }
            Does::Refuses => {
                let refused = call(&agent, name, input, cx).unwrap_err();
                assert_eq!(refused.code, ErrorCode::ConsentRequired, "{name}: {refused:?}");
                assert!(!refused.retryable, "{name}");
                assert_eq!(
                    refused.message,
                    format!(
                        "Pane {pane} is the person's pinned page. Call browser_claim with pane {pane} to ask them, or browser_duplicate to ask for a copy in your worktree."
                    )
                );
            }
            Does::Asks => {
                let asking = send(&agent, None, op(name, input), cx);
                cx.run_until_parked();
                assert!(cx.has_pending_prompt(), "{name} asks the person");
                cx.simulate_prompt_answer("Cancel");
                cx.run_until_parked();
                let declined = taken(&asking).expect("answered").unwrap_err();
                assert_eq!(declined.code, ErrorCode::UserDeclined, "{name}");
                cx.executor().advance_clock(DECLINED_FOR);
            }
        }
        assert!(!cx.has_pending_prompt(), "{name} left no prompt open");
    }
    let info = listed(&agent, pane, cx);
    assert_eq!(
        (
            info["place"].clone(),
            info["persons"].clone(),
            info["worktree"].clone(),
            info["consent"].clone(),
        ),
        (json!("pinned"), json!(true), Value::Null, json!("none"))
    );
}

#[gpui_kit::test]
fn a_call_naming_no_pane_never_means_the_persons_page(cx: &mut TestAppContext) {
    let root = repo("control-persons-unnamed", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-unnamed", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let theirs = persons_page(&rig, &agent, "https://mail.example/", true, cx);
    rig.shell.update(cx, |shell, cx| {
        assert!(shell.show_tab_holding(theirs, cx));
    });
    cx.run_until_parked();
    for name in ["browser_claim", "browser_read", "browser_release"] {
        let refused = call(&agent, name, json!({}), cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound, "{name}: {}", refused.message);
    }
    assert!(!cx.has_pending_prompt());

    let opened = json!({ "kind": "browser", "url": "https://example.com/" });
    let mine = call(&agent, "tab_open", opened, cx).unwrap()["pane"].clone();
    allowed(&agent, theirs, cx);
    let claimed = call(&agent, "browser_claim", json!({}), cx).unwrap();
    assert_eq!(claimed, json!({ "pane": mine, "held": true }));
    assert_eq!(reading(&agent, theirs, cx).code, ErrorCode::BrowserUnavailable);
    let shown = read_shell(&rig, cx, |shell, _| shell.workspace.apart_shown_key());
    assert_eq!(shown, Some(theirs), "the person's page is the tab shown");
    let released = call(&agent, "browser_release", json!({}), cx).unwrap();
    assert_eq!(released, json!({ "pane": mine, "held": false }));
    assert_eq!(listed(&agent, theirs, cx)["consent"], json!("granted"));
}

#[gpui_kit::test]
fn consent_is_one_clients_on_one_connection_for_one_page_while_its_lease_lasts(
    cx: &mut TestAppContext,
) {
    let root = repo("control-persons-scope", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-scope", &[&root], cx);
    select(&rig.shell, &root, cx);
    let (agent, other) = (terminal("term_1"), terminal("term_2"));
    let pinned = persons_page(&rig, &agent, "https://mail.example/", true, cx);
    let loose = persons_page(&rig, &agent, "https://bank.example/", false, cx);
    allowed(&agent, pinned, cx);

    assert_eq!(reading(&agent, pinned, cx).code, ErrorCode::BrowserUnavailable);
    let mine = listed(&agent, pinned, cx);
    assert_eq!(
        (mine["consent"].clone(), mine["controller"].clone()),
        (json!("granted"), json!("you"))
    );
    assert_eq!(reading(&other, pinned, cx).code, ErrorCode::ConsentRequired);
    let theirs = listed(&other, pinned, cx);
    assert_eq!(
        (theirs["consent"].clone(), theirs["controller"].clone()),
        (json!("none"), json!("agent"))
    );
    assert_eq!(reading(&agent, loose, cx).code, ErrorCode::ConsentRequired);
    let loose_page = listed(&agent, loose, cx);
    assert_eq!(
        (loose_page["place"].clone(), loose_page["consent"].clone()),
        (json!("loose"), json!("none"))
    );
    let view = read_shell(&rig, cx, |shell, _| shell.workspace.pane(pinned)?.view().cloned());
    let alert = koyo_browser_ipc::Dialog::Alert {
        message: "Your session ends soon".into(),
    };
    view.expect("the pinned page").update(cx, |view, cx| view.page_asks(alert, cx));
    cx.run_until_parked();
    assert_eq!(listed(&agent, pinned, cx)["dialog"]["kind"], json!("alert"));
    assert_eq!(listed(&other, pinned, cx)["dialog"], Value::Null);
    let elsewhere = Caller {
        connection: Some(9),
        ..agent.clone()
    };
    assert_eq!(reading(&elsewhere, pinned, cx).code, ErrorCode::ConsentRequired);
    assert_eq!(listed(&elsewhere, pinned, cx)["consent"], json!("none"));
    assert_eq!(reading(&agent, pinned, cx).code, ErrorCode::BrowserUnavailable);
    let again = call(&agent, "browser_claim", json!({ "pane": pinned }), cx).unwrap();
    assert_eq!(again, json!({ "pane": pinned, "held": true }));
    assert!(!cx.has_pending_prompt());
    for (name, input) in [
        ("tab_select", json!({ "pane": pinned })),
        ("pane_focus", json!({ "pane": pinned })),
        ("tab_close", json!({ "pane": pinned })),
        ("pane_split", json!({ "pane": pinned, "edge": "right", "kind": "terminal" })),
    ] {
        let refused = call(&agent, name, input, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound, "{name}");
        assert!(
            (refused.message).starts_with(&format!("Pane {pinned} is the person's pinned page")),
            "{name}: {}",
            refused.message
        );
    }
    assert_eq!(read_shell(&rig, cx, |shell, _| shell.workspace.pinned().len()), 1);

    let released = call(&agent, "browser_release", json!({ "pane": pinned }), cx).unwrap();
    assert_eq!(released, json!({ "pane": pinned, "held": false }));
    assert_eq!(reading(&agent, pinned, cx).code, ErrorCode::ConsentRequired);
    assert_eq!(listed(&agent, pinned, cx)["controller"], Value::Null);
}

#[gpui_kit::test]
fn a_client_that_may_no_longer_drive_pages_loses_its_consent(cx: &mut TestAppContext) {
    let root = repo("control-persons-scoped", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-scoped", &[&root], cx);
    select(&rig.shell, &root, cx);
    let page = persons_page(&rig, &terminal("term_0"), "https://mail.example/", true, cx);
    allow("Claude Code", &Scope::DEFAULT_GRANT, cx);
    let client = external("Claude Code");
    allowed(&client, page, cx);
    assert_eq!(reading(&client, page, cx).code, ErrorCode::BrowserUnavailable);
    let reads = vec![Scope::Read, Scope::Navigate];
    cx.update(|cx| crate::control::set_client_scopes("Claude Code", reads, cx));
    cx.run_until_parked();
    assert_eq!(listed(&client, page, cx)["controller"], Value::Null);
    assert_eq!(reading(&client, page, cx).code, ErrorCode::ScopeDenied);
    let everything = Scope::DEFAULT_GRANT.to_vec();
    cx.update(|cx| crate::control::set_client_scopes("Claude Code", everything, cx));
    assert_eq!(reading(&client, page, cx).code, ErrorCode::ConsentRequired, "it ended");
    assert_eq!(listed(&client, page, cx)["controller"], Value::Null, "let go");
}

#[gpui_kit::test]
fn two_connections_of_one_name_dont_share_the_persons_consent(cx: &mut TestAppContext) {
    let root = repo("control-persons-twins", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-twins", &[&root], cx);
    select(&rig.shell, &root, cx);
    let page = persons_page(&rig, &terminal("term_0"), "https://mail.example/", true, cx);
    let first = terminal("term_1");
    let second = Caller {
        connection: Some(42),
        ..first.clone()
    };
    allowed(&first, page, cx);
    assert_eq!(listed(&first, page, cx)["controller"], json!("you"));
    assert_eq!(listed(&second, page, cx)["controller"], json!("agent"));
    let released = call(&second, "browser_release", json!({ "pane": page }), cx).unwrap();
    assert_eq!(released["held"], json!(false));
    assert_eq!(reading(&first, page, cx).code, ErrorCode::BrowserUnavailable);
    let refused = call(&second, "browser_claim", json!({ "pane": page }), cx).unwrap_err();
    assert_eq!((refused.code, refused.message.as_str()), (ErrorCode::ControlRequired, ANOTHER));
    assert!(!cx.has_pending_prompt());
    let claim = json!({ "pane": page, "takeover": true });
    let asking = send(&second, None, op("browser_claim", claim), cx);
    cx.run_until_parked();
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert_eq!(taken(&asking).expect("answered").unwrap()["held"], json!(true));
    assert_eq!(reading(&first, page, cx).code, ErrorCode::ConsentRequired);
    assert_eq!(listed(&first, page, cx)["controller"], json!("agent"));
    assert_eq!(listed(&second, page, cx)["controller"], json!("you"));
}

/// How a claim of the person's page another client holds is refused: naming nobody.
const ANOTHER: &str =
    "Another agent holds this page; browser_claim with takeover asks the person for it.";

#[gpui_kit::test]
fn a_refusal_on_the_persons_page_names_no_other_client(cx: &mut TestAppContext) {
    let root = repo("control-persons-nameless", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-nameless", &[&root], cx);
    select(&rig.shell, &root, cx);
    let page = persons_page(&rig, &terminal("term_0"), "https://mail.example/", true, cx);
    allowed(&terminal("term_1"), page, cx);
    let refused = call(&terminal("term_2"), "browser_claim", json!({ "pane": page }), cx);
    let refused = refused.unwrap_err();
    assert_eq!((refused.code, refused.message.as_str()), (ErrorCode::ControlRequired, ANOTHER));
    assert!(!cx.has_pending_prompt());
}

#[gpui_kit::test]
fn a_call_with_no_connection_to_bind_consent_to_isnt_asked_about(cx: &mut TestAppContext) {
    let root = repo("control-persons-nowhere", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-nowhere", &[&root], cx);
    select(&rig.shell, &root, cx);
    let page = persons_page(&rig, &terminal("term_0"), "https://mail.example/", true, cx);
    let nowhere = Caller {
        connection: None,
        ..terminal("term_8")
    };
    for name in ["browser_claim", "browser_duplicate"] {
        let refused = call(&nowhere, name, json!({ "pane": page }), cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{name}: {}", refused.message);
        assert!(!cx.has_pending_prompt());
    }
}
