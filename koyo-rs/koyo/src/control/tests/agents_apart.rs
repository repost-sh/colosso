//! Agents' pages kept apart from the person's (`docs/browser.md` 15.7): a page an agent's tool
//! opened is the agent's, whatever agents' pages are signed in as; one the person opened in a
//! worktree is the worktree's while agents' pages are signed in as the person, and the person's
//! to let an agent use, as a pinned or a loose page is, while they're kept apart. The tests'
//! engine never starts, so a call let through fails `browser_unavailable` after the gate.

use gpui_kit::TestAppContext;
use koyo_app_control::ErrorCode;
use serde_json::json;

use super::persons::{allowed, listed, reading};
use super::{call, controlled, terminal};
use crate::browser::profiles::{self, AgentPages};
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
            .read_with(cx, |shell, _| shell.workspace.pane(pane).unwrap().by_agent)
    };
    assert!(by_agent(own, cx) && !by_agent(persons, cx));

    // Signed in as the person, both are the worktree's: neither asks.
    assert_eq!(reading(&agent, own, cx).code, ErrorCode::BrowserUnavailable);
    assert_eq!(
        reading(&agent, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );
    assert_eq!(listed(&agent, persons, cx)["persons"], json!(false));

    // Kept apart, the person's is theirs to ask for, and the agent's is still its own.
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
    // A call naming no page never means it.
    let unnamed = call(&agent, "browser_read", json!({}), cx).unwrap_err();
    assert_ne!(unnamed.code, ErrorCode::ConsentRequired);

    // Allowed, the consent holds as a tab before it closes and it moves up the list.
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

    // Signed in as the person again, it's the worktree's, for any agent.
    agents(AgentPages::Shared, cx);
    let other = terminal("term_2");
    assert_eq!(
        reading(&other, persons, cx).code,
        ErrorCode::BrowserUnavailable
    );
}

#[gpui_kit::test]
fn an_agents_page_is_an_agents_from_its_making_and_once_reopened(cx: &mut TestAppContext) {
    use crate::browser::profiles::By;

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
            (pane.by_agent, pane.view().map(|view| view.read(cx).by()))
        })
    };
    // Marked as it's made, signed in as the person or not, so a switch later moves it.
    assert_eq!(made(own, cx), (true, Some(By::Agent)));
    call(&agent, "tab_close", json!({ "pane": own }), cx).unwrap();
    cx.run_until_parked();
    let reopened = call(&agent, "tab_reopen", json!({}), cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    assert_eq!(made(reopened, cx), (true, Some(By::Agent)));
}
