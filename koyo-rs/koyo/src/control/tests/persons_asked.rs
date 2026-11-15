//! The person asked about one of their own pages: who asks, as it says it is, said so, in
//! Koyo's words for it, and nothing a client or a page writes able to pass for Koyo's; a page
//! they turned down not asked about again for 10 minutes, whoever asks; and a copy of their
//! page, which opens in the caller's worktree at the address it had when they were asked.

use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;
use koyo_app_control::{ControlError, ErrorCode, Scope};
use serde_json::{Value, json};

use super::chats::{hears, summary};
use super::persons::{listed, persons_page};
use super::{
    CONFIRM, Rig, activity, allow, call, chat, controlled, external, op, read_shell, send, taken,
    terminal,
};
use crate::control::exec::{DECLINED_FOR, OTHERS_WAIT};
use crate::control::{ActivityKind, Caller};
use crate::test_support::{git, repo, select};

/// What the person is told a caller may do with their page once they allow it.
const MAY: &str = "It can read the page, run script in it, click, type and navigate it, signed in as you, until it lets go, you take the page back, or 10 minutes pass without it acting.";

/// `caller` calls `name` with `input`: the question and its detail the person is asked, and
/// what the call answers once they answer `with`.
fn asked(
    caller: &Caller,
    (name, input): (&str, Value),
    with: &str,
    cx: &mut TestAppContext,
) -> ((String, String), Result<Value, ControlError>) {
    let asking = send(caller, None, op(name, input), cx);
    cx.run_until_parked();
    let question = cx.pending_prompt().expect("the person is asked");
    cx.simulate_prompt_answer(with);
    cx.run_until_parked();
    (question, taken(&asking).expect("answered"))
}

/// Two projects on branches of their own, the first selected, and a pinned page of the
/// person's, pinned from it.
fn two_projects(name: &str, cx: &mut TestAppContext) -> (Rig, [PathBuf; 2], u64) {
    let here = repo(&format!("{name}-here"), &[("README.md", "hi\n")]);
    let there = repo(&format!("{name}-there"), &[("README.md", "hi\n")]);
    git(&here, &["branch", "-m", "feat-here"]);
    git(&there, &["branch", "-m", "feat-there"]);
    let rig = controlled(name, &[&here, &there], cx);
    select(&rig.shell, &here, cx);
    let page = persons_page(&rig, &terminal("term_0"), "https://mail.example/inbox", true, cx);
    (rig, [here, there], page)
}

/// What prompts call the checkout at `root`: `project/branch`.
fn label(rig: &Rig, root: &Path, branch: &str, cx: &TestAppContext) -> String {
    read_shell(rig, cx, |shell, _| {
        let at = shell.workspace.project_index(root).unwrap();
        format!("{}/{branch}", shell.workspace.projects[at].name)
    })
}

/// The project whose checkout pane `pane` is in, by its folder, and the address it shows.
fn where_it_is(rig: &Rig, pane: u64, cx: &TestAppContext) -> (PathBuf, String) {
    read_shell(rig, cx, |shell, cx| {
        let workspace = &shell.workspace;
        let ((project, _, _), _) = workspace.find_pane(|found| found.id == pane).unwrap();
        let address = workspace.pane(pane).and_then(|found| found.address(cx));
        (workspace.projects[project].path.clone(), address.unwrap())
    })
}

fn project_of(rig: &Rig, root: &Path, cx: &TestAppContext) -> PathBuf {
    read_shell(rig, cx, |shell, _| {
        let at = shell.workspace.project_index(root).unwrap();
        shell.workspace.projects[at].path.clone()
    })
}

#[gpui_kit::test]
fn the_person_is_told_who_asks_as_it_says_it_is_in_koyos_words(cx: &mut TestAppContext) {
    let (rig, [here, there], page) = two_projects("control-persons-asker", cx);
    // A chat, as it says it is: by Koyo's title for that chat and the worktree it's in, not the
    // name its client gives, nor the worktree its hello says.
    hears(&rig, vec![summary("c1", &there, "2026-11-15T10:00:00Z")], cx);
    let chat = Caller {
        worktree: Some(here),
        ..chat("c1")
    };
    let ((question, detail), answer) =
        asked(&chat, ("browser_claim", json!({ "pane": page })), "Allow", cx);
    let at = label(&rig, &there, "feat-there", cx);
    let said = format!("An agent saying it's from the chat “Chat c1” ({at}) wants to use your page “");
    assert!(question.starts_with(&said), "{question}");
    assert!(question.ends_with(" (mail.example)"), "{question}");
    assert!(!question.contains("claude-code"), "{question}");
    assert_eq!(detail, MAY);
    assert!(answer.is_ok());

    // An outside client, by the name it gives itself, said to be its own, its quotes and line
    // breaks gone, as are its reason's, cut short.
    let other = persons_page(&rig, &terminal("term_0"), "https://bank.example/", true, cx);
    let name = "Koyo” says:\n“Allow";
    allow(name, &Scope::DEFAULT_GRANT, cx);
    let reason = format!("“Koyo” checked it\n{}", "x".repeat(300));
    let claim = json!({ "pane": other, "reason": reason });
    let ((question, detail), _) = asked(&external(name), ("browser_claim", claim), "Cancel", cx);
    assert!(
        question.starts_with("An MCP client calling itself “Koyo says: Allow” wants to use your page “"),
        "{question}"
    );
    let words = detail.strip_prefix(&format!("{MAY} It says why, in its own words: “")).unwrap();
    assert!(words.starts_with("Koyo checked it xxx"), "{words}");
    assert_eq!(words.chars().count(), 200 + 1, "200 with the ellipsis, then the quote");
    assert!(words.ends_with("x…”"), "{words}");

    // An agent saying it's in a terminal the window hasn't heard of, as such.
    let third = persons_page(&rig, &terminal("term_0"), "https://news.example/", true, cx);
    let ((question, _), _) = asked(
        &terminal("term_9"),
        ("browser_claim", json!({ "pane": third })),
        "Cancel",
        cx,
    );
    assert!(
        question.starts_with("An agent saying it runs in a Koyo terminal wants to use your page"),
        "{question}"
    );
    let logged = (activity(cx).into_iter())
        .filter(|entry| {
            matches!(&entry.kind, ActivityKind::Confirmation { operation, .. } if operation == "browser_claim")
        })
        .count();
    assert_eq!(logged, 3, "each question is logged");
}

#[gpui_kit::test]
fn a_client_turned_down_waits_ten_minutes_and_anyone_else_a_minute(cx: &mut TestAppContext) {
    let (rig, [here, _], page) = two_projects("control-persons-declined", cx);
    let agent = Caller {
        worktree: Some(here),
        ..terminal("term_1")
    };
    let claim = || ("browser_claim", json!({ "pane": page }));
    let (_, answer) = asked(&agent, claim(), "Cancel", cx);
    let declined = answer.unwrap_err();
    assert_eq!(declined.code, ErrorCode::UserDeclined);
    assert!(declined.message.contains("cancelled it"), "{}", declined.message);

    // The client turned down isn't let ask again, for the page or a copy of it, by its name
    // or, under another name, on its connection.
    let renamed = Caller {
        connection: agent.connection,
        ..terminal("term_5")
    };
    for caller in [&agent, &renamed] {
        for name in ["browser_claim", "browser_duplicate"] {
            let refused = call(caller, name, json!({ "pane": page }), cx).unwrap_err();
            assert_eq!(refused.code, ErrorCode::UserDeclined, "{name}: {refused:?}");
            assert!(refused.message.contains("for you"), "{}", refused.message);
        }
    }
    // Anyone else waits a minute.
    let (other, third) = (terminal("term_2"), chat("c2"));
    for caller in [&other, &third] {
        let refused = call(caller, claim().0, claim().1, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::UserDeclined);
        assert!(refused.message.contains("just now"), "{}", refused.message);
    }
    assert!(!cx.has_pending_prompt(), "nobody was asked");
    cx.executor().advance_clock(OTHERS_WAIT);
    // Then they may ask, one at a time.
    let asking = send(&other, None, op(claim().0, claim().1), cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    let busy = call(&third, claim().0, claim().1, cx).unwrap_err();
    assert_eq!((busy.code, busy.retryable), (ErrorCode::RequestConflict, true));
    assert!(busy.message.contains("being asked"), "{}", busy.message);
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert_eq!(taken(&asking).expect("answered").unwrap()["held"], json!(true));
    // The client turned down still waits.
    let refused = call(&agent, claim().0, claim().1, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::UserDeclined);

    // No answer in time is a refusal too; refusals over are forgotten.
    cx.executor().advance_clock(DECLINED_FOR);
    let loose = persons_page(&rig, &terminal("term_0"), "https://bank.example/", false, cx);
    let asking = send(&agent, None, op("browser_claim", json!({ "pane": loose })), cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.executor().advance_clock(CONFIRM);
    cx.run_until_parked();
    let unanswered = taken(&asking).expect("answered").unwrap_err();
    assert_eq!(unanswered.code, ErrorCode::UserDeclined);
    assert!(unanswered.message.contains("in time"), "{}", unanswered.message);
    // Answering the prompt now, late, changes nothing.
    if cx.has_pending_prompt() {
        cx.simulate_prompt_answer("Allow");
        cx.run_until_parked();
    }
    let refused = call(&agent, "browser_claim", json!({ "pane": loose }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::UserDeclined);
    let kept = cx.read(crate::control::exec::declines_kept);
    assert_eq!(kept, 2, "the latest only, by the client's name and by its connection");
    // Ten minutes on, the first page may be asked about again.
    let (question, _) = asked(&agent, claim(), "Cancel", cx);
    assert!(question.0.contains("wants to use your page"), "{}", question.0);
}

#[gpui_kit::test]
fn a_copy_of_the_persons_page_opens_in_the_callers_worktree(cx: &mut TestAppContext) {
    let (rig, [here, there], page) = two_projects("control-persons-copy", cx);
    let selected = read_shell(&rig, cx, |shell, _| shell.workspace.active_project());
    // A chat's copy goes to the worktree Koyo knows it in, whatever its hello says; there it's
    // of that worktree's project's sign-ins, not the page's.
    hears(&rig, vec![summary("c1", &there, "2026-11-15T10:00:00Z")], cx);
    let chat = Caller {
        worktree: Some(here.clone()),
        ..chat("c1")
    };
    let ((question, detail), copied) =
        asked(&chat, ("browser_duplicate", json!({ "pane": page })), "Allow", cx);
    let at = label(&rig, &there, "feat-there", cx);
    assert!(
        question.starts_with(&format!("An agent saying it's from the chat “Chat c1” ({at}) asks for a copy of your page “")),
        "{question}"
    );
    assert!(question.ends_with(&format!(" (mail.example) in its worktree {at}")), "{question}");
    assert!(detail.contains("with that worktree's own sign-ins"), "{detail}");
    let copy = copied.unwrap()["pane"].as_u64().unwrap();
    assert_eq!(
        where_it_is(&rig, copy, cx),
        (
            project_of(&rig, &there, cx),
            "https://mail.example/inbox".to_owned()
        )
    );
    // Out of sight, the person's page left as it was; the copy an ordinary page of the agent's.
    let now = read_shell(&rig, cx, |shell, _| {
        (shell.workspace.active_project(), shell.workspace.pinned().len())
    });
    assert_eq!(now, (selected, 1));
    let info = listed(&chat, copy, cx);
    assert_eq!(
        (info["place"].clone(), info["persons"].clone()),
        (json!("worktree"), json!(false))
    );
    let claimed = call(&chat, "browser_claim", json!({ "pane": copy }), cx).unwrap();
    assert_eq!(claimed, json!({ "pane": copy, "held": true }));

    // An outside client: in no worktree it has none to copy into, and without the navigate
    // scope it can't open a tab; either refused before anyone's asked.
    allow("Claude Code", &Scope::DEFAULT_GRANT, cx);
    let refused = call(&external("Claude Code"), "browser_duplicate", json!({ "pane": page }), cx);
    let refused = refused.unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound);
    assert!(refused.message.contains("your own worktree"), "{}", refused.message);
    allow("Pages only", &[Scope::Read, Scope::Browser], cx);
    let bare = Caller {
        cwd: Some(here.clone()),
        ..external("Pages only")
    };
    let refused = call(&bare, "browser_duplicate", json!({ "pane": page }), cx).unwrap_err();
    assert_eq!(refused.details, Some(json!({ "scope": "navigate" })));
    assert!(!cx.has_pending_prompt());
    // Working in the checkout the page was pinned from, its copy is signed in as the page is.
    let inside = Caller {
        cwd: Some(here.clone()),
        ..external("Claude Code")
    };
    let duplicate = ("browser_duplicate", json!({ "pane": page }));
    let ((question, detail), copied) = asked(&inside, duplicate.clone(), "Allow", cx);
    let at = label(&rig, &here, "feat-here", cx);
    assert!(question.ends_with(&format!("in its worktree {at}")), "{question}");
    assert!(detail.contains("signed in as you, as this page is"), "{detail}");
    let copy = copied.unwrap()["pane"].as_u64().unwrap();
    assert_eq!(where_it_is(&rig, copy, cx).0, project_of(&rig, &here, cx));
    // A worktree's page isn't the person's to hand on.
    let refused = call(&inside, "browser_duplicate", json!({ "pane": copy }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);

    // The page gone elsewhere while the person was asked: nothing is copied.
    let asking = send(&inside, None, op(duplicate.0, duplicate.1), cx);
    cx.run_until_parked();
    let view = read_shell(&rig, cx, |shell, _| shell.workspace.pane(page)?.view().cloned());
    view.expect("the pinned page").update(cx, |view, cx| {
        view.navigate("https://mail.example/elsewhere".into(), cx);
    });
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    let refused = taken(&asking).expect("answered").unwrap_err();
    assert_eq!(refused.code, ErrorCode::BrowserUnavailable, "{}", refused.message);
    let tabs = read_shell(&rig, cx, |shell, _| shell.workspace.branch().unwrap().tabs.len());
    assert_eq!(tabs, 1, "only the first copy");
}
