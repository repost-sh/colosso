//! Agents' pages kept apart from the person's (`docs/browser.md` 15.7): a page an agent's tool
//! opened is the agent's, whatever agents' pages are signed in as; one the person opened in a
//! worktree is the worktree's while agents' pages are signed in as the person, and the person's
//! to let an agent use, as a pinned or a loose page is, while they're kept apart. The tests'
//! engine never starts, so a call let through fails `browser_unavailable` after the gate.

use std::time::Duration;

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::{Value, json};

use super::persons::{allowed, listed, reading};
use super::{call, controlled, op, send, taken, terminal};
use crate::browser::profiles::{self, AgentPages, By};
use crate::test_support::{open_page, repo, select};

fn agents(pages: AgentPages, cx: &mut TestAppContext) {
    cx.update(|cx| profiles::update(cx, |profiles| profiles.agents = pages));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn kept_apart_a_page_the_person_opened_in_a_worktree_is_theirs_to_let_an_agent_use(
    cx: &mut TestAppContext,
) {
    let root = repo("control-agents-apart", &[("README.md", "hi\n")]);
    let rig = controlled("control-agents-apart", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opened = json!({ "kind": "browser", "url": "https://agent.example/" });
    let own = call(&agent, "tab_open", opened, cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    let persons = open_page(rig.window, &rig.shell, Some("https://mail.example/"), cx);
    cx.run_until_parked();
    let by_agent = |pane: u64, cx: &mut TestAppContext| {
        rig.shell
            .read_with(cx, |shell, _| shell.workspace.pane(pane).unwrap().by() == By::Agent)
    };
    assert!(by_agent(own, cx) && !by_agent(persons, cx));

    assert_eq!(reading(&agent, own, cx).code, ErrorCode::BrowserUnavailable);
    assert_eq!(
        reading(&agent, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );
    assert_eq!(listed(&agent, persons, cx)["persons"], json!(false));

    agents(AgentPages::Separate, cx);
    assert_eq!(reading(&agent, own, cx).code, ErrorCode::BrowserUnavailable);
    let refused = reading(&agent, persons, cx);
    assert_eq!(refused.code, ErrorCode::ConsentRequired);
    assert!(
        refused.message.contains("person's worktree page"),
        "{}",
        refused.message
    );
    let info = listed(&agent, persons, cx);
    assert_eq!(
        (info["place"].clone(), info["persons"].clone()),
        (json!("worktree"), json!(true))
    );
    let unnamed = call(&agent, "browser_read", json!({}), cx).unwrap_err();
    assert_ne!(unnamed.code, ErrorCode::ConsentRequired);

    let question = allowed(&agent, persons, cx);
    assert!(question.contains("wants to use your page"), "{question}");
    assert_eq!(
        reading(&agent, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );
    call(&agent, "tab_close", json!({ "pane": own }), cx).unwrap();
    cx.run_until_parked();
    assert_eq!(
        reading(&agent, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );

    agents(AgentPages::Shared, cx);
    let other = terminal("term_2");
    assert_eq!(
        reading(&other, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );
}

#[gpui_kit::test]
fn an_agents_page_is_an_agents_from_its_making_and_once_reopened(cx: &mut TestAppContext) {
    let root = repo("control-agents-reopened", &[("README.md", "hi\n")]);
    let rig = controlled("control-agents-reopened", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opened = json!({ "kind": "browser", "url": "https://agent.example/" });
    let own = call(&agent, "tab_open", opened, cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    let made = |pane: u64, cx: &mut TestAppContext| {
        rig.shell.read_with(cx, |shell, cx| {
            let pane = shell.workspace.pane(pane).unwrap();
            (pane.by() == By::Agent, pane.view().map(|view| view.read(cx).by()))
        })
    };
    assert_eq!(made(own, cx), (true, Some(By::Agent)));
    call(&agent, "tab_close", json!({ "pane": own }), cx).unwrap();
    cx.run_until_parked();
    let reopened = call(&agent, "tab_reopen", json!({}), cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    assert_eq!(made(reopened, cx), (true, Some(By::Agent)));
}

/// As agents' pages come to be kept apart, a page the person opened in a worktree becomes
/// theirs: an agent holding it lets it go and its call under way there stops, while the page
/// the agent's own tool opened stays held.
#[gpui_kit::test]
fn kept_apart_the_persons_worktree_page_an_agent_held_is_let_go_and_its_own_kept(
    cx: &mut TestAppContext,
) {
    let root = repo("control-agents-apart-held", &[("README.md", "hi\n")]);
    let rig = controlled("control-agents-apart-held", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opened = json!({ "kind": "browser", "url": "https://agent.example/" });
    let own = call(&agent, "tab_open", opened, cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    let persons = open_page(rig.window, &rig.shell, Some("https://mail.example/"), cx);
    cx.run_until_parked();
    for pane in [own, persons] {
        let held = call(&agent, "browser_claim", json!({ "pane": pane }), cx).unwrap();
        assert_eq!(held["held"], json!(true));
    }
    let wait = json!({ "pane": persons, "for": "url", "value": "never", "timeoutMs": 20_000 });
    let waiting = send(&agent, None, op("browser_wait", wait), cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();

    agents(AgentPages::Separate, cx);
    rig.shell.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(listed(&agent, persons, cx)["controller"], Value::Null, "the person's, let go");
    assert_ne!(listed(&agent, own, cx)["controller"], Value::Null, "its own, still held");
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let stopped = taken(&waiting).expect("answered").unwrap_err();
    assert_eq!(stopped.code, ErrorCode::ConsentRequired, "{}", stopped.message);
}
