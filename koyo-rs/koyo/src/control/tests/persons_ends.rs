//! How the person's consent for one of their pages ends: as the page moves, closes, is pinned
//! again or at another address; unused for 10 minutes; with the connection it was given on; as
//! the person takes the page, after which no lease of the client's brings it back; and in the
//! middle of a call, which then answers `consent_required`.

use std::time::Duration;

use gpui_kit::{AppContext as _, TestAppContext};
use koyo_app_control::ErrorCode;
use serde_json::{Value, json};

use super::persons::{allowed, listed, persons_page, reading};
use super::{activity, call, controlled, op, read_shell, send, taken, terminal};
use crate::control::ActivityKind;
use crate::browser::BrowserEvent;
use crate::browser::control::IMPLICIT_HOLD;
use crate::control::exec::IDLE;
use crate::control::{Caller, Request, handle};
use crate::test_support::{click_pane, draw, repo, select};

/// `agent`'s consent for `pane` is over, and its lease of the page with it.
fn ended(agent: &Caller, pane: u64, cx: &mut TestAppContext) {
    assert_eq!(reading(agent, pane, cx).code, ErrorCode::ConsentRequired);
    assert_eq!(listed(agent, pane, cx)["controller"], Value::Null, "let go");
}

/// Connection `connection` opens, or closes.
fn present(connection: u64, open: bool, cx: &mut TestAppContext) {
    cx.update(|cx| handle(Request::Present { connection, open }, cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn consent_ends_as_the_page_moves_closes_or_is_pinned_elsewhere(cx: &mut TestAppContext) {
    let root = repo("control-persons-moves", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-moves", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");

    let page = persons_page(&rig, &agent, "https://a.example/", true, cx);
    allowed(&agent, page, cx);
    rig.shell.update(cx, |shell, cx| shell.unpin(page, cx));
    cx.run_until_parked();
    let ordinary = listed(&agent, page, cx);
    assert_eq!(
        (ordinary["place"].clone(), ordinary["persons"].clone()),
        (json!("worktree"), json!(false))
    );
    assert_eq!(ordinary["controller"], Value::Null, "its lease went with the consent");

    let page = persons_page(&rig, &agent, "https://b.example/", false, cx);
    allowed(&agent, page, cx);
    rig.shell.update(cx, |shell, cx| assert!(shell.pin_tab_of(page, usize::MAX, cx)));
    cx.run_until_parked();
    ended(&agent, page, cx);

    allowed(&agent, page, cx);
    rig.shell.update(cx, |shell, cx| {
        shell.repin_at(page, "https://b.example/other".into(), cx);
    });
    cx.run_until_parked();
    ended(&agent, page, cx);

    allowed(&agent, page, cx);
    rig.shell.update(cx, |shell, cx| shell.close_pinned(page, cx));
    cx.run_until_parked();
    assert_eq!(listed(&agent, page, cx)["open"], json!(false));
    let window = rig.window;
    cx.update_window(window, |_, window, cx| {
        rig.shell.update(cx, |shell, cx| {
            assert!(shell.show_tab_holding(page, cx));
            shell.make_shown(window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(listed(&agent, page, cx)["consent"], json!("none"));
    assert_eq!(reading(&agent, page, cx).code, ErrorCode::ConsentRequired);
}

#[gpui_kit::test]
fn consent_ends_unused_or_with_the_connection_it_was_given_on(cx: &mut TestAppContext) {
    let root = repo("control-persons-lapses", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-lapses", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let page = persons_page(&rig, &agent, "https://a.example/", true, cx);

    allowed(&agent, page, cx);
    cx.executor().advance_clock(IDLE / 2);
    assert_eq!(reading(&agent, page, cx).code, ErrorCode::BrowserUnavailable);
    cx.executor().advance_clock(IDLE * 3 / 4);
    cx.run_until_parked();
    assert_eq!(listed(&agent, page, cx)["consent"], json!("granted"));
    cx.executor().advance_clock(IDLE / 2);
    cx.run_until_parked();
    ended(&agent, page, cx);

    let connected = Caller {
        connection: Some(7),
        ..agent.clone()
    };
    present(7, true, cx);
    allowed(&connected, page, cx);
    assert_eq!(reading(&connected, page, cx).code, ErrorCode::BrowserUnavailable);
    present(7, false, cx);
    ended(&connected, page, cx);

    let leaving = Caller {
        connection: Some(8),
        ..agent.clone()
    };
    present(8, true, cx);
    let asking = send(&leaving, None, op("browser_claim", json!({ "pane": page })), cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    present(8, false, cx);
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    let refused = taken(&asking).expect("answered").unwrap_err();
    assert_eq!(refused.code, ErrorCode::NotFound, "{}", refused.message);
    assert!(refused.message.contains("had closed"), "{}", refused.message);
    assert_eq!(listed(&leaving, page, cx)["controller"], Value::Null);
    let logged: Vec<_> = (activity(cx).into_iter().take(2))
        .map(|entry| match entry.kind {
            ActivityKind::Confirmation { decision, .. } => format!("{decision:?}"),
            ActivityKind::Call { outcome, .. } => format!("{:?}", outcome.map_err(|error| error.code)),
            ActivityKind::Approval(_) => "approval".to_owned(),
        })
        .collect();
    assert_eq!(logged, ["Err(NotFound)", "Allowed"]);
}

#[gpui_kit::test]
fn consent_ends_as_the_person_takes_the_page_and_no_lease_brings_it_back(
    cx: &mut TestAppContext,
) {
    let root = repo("control-persons-taken", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-taken", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let page = persons_page(&rig, &agent, "https://a.example/", true, cx);
    rig.shell.update(cx, |shell, cx| assert!(shell.show_tab_holding(page, cx)));
    draw(rig.window, cx);
    allowed(&agent, page, cx);

    click_pane(rig.window, page, cx);
    assert_eq!(listed(&agent, page, cx)["controller"], json!("person"));
    cx.executor().advance_clock(IMPLICIT_HOLD + Duration::from_millis(100));
    cx.run_until_parked();
    assert_eq!(listed(&agent, page, cx)["controller"], Value::Null);
    let view = read_shell(&rig, cx, |shell, _| shell.workspace.pane(page)?.view().cloned());
    let actor = agent.actor();
    (view.expect("the page")).update(cx, |view, cx| {
        view.agent_claims(actor, false, None, cx).unwrap();
    });
    assert_eq!(listed(&agent, page, cx)["controller"], json!("agent"), "under no consent");
    assert_eq!(reading(&agent, page, cx).code, ErrorCode::ConsentRequired);
    let released = call(&agent, "browser_release", json!({ "pane": page }), cx).unwrap();
    assert_eq!(released["held"], json!(false));
    let click = json!({ "pane": page, "x": 1, "y": 1 });
    let refused = call(&agent, "browser_click", click, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ConsentRequired);
    ended(&agent, page, cx);
    allowed(&agent, page, cx);
    assert_eq!(listed(&agent, page, cx)["controller"], json!("you"));
}

#[gpui_kit::test]
fn a_call_under_way_stops_once_consent_ends_or_its_page_closes(cx: &mut TestAppContext) {
    let root = repo("control-persons-midway", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-midway", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let page = persons_page(&rig, &agent, "https://a.example/", true, cx);
    allowed(&agent, page, cx);
    let wait = json!({ "pane": page, "for": "url", "value": "never", "timeoutMs": 20_000 });
    let waiting = send(&agent, None, op("browser_wait", wait), cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(taken(&waiting).is_none(), "still waiting");
    rig.shell.update(cx, |shell, cx| shell.unpin(page, cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let stopped = taken(&waiting).expect("answered").unwrap_err();
    assert_eq!(stopped.code, ErrorCode::ConsentRequired, "{}", stopped.message);
    assert_eq!(listed(&agent, page, cx)["controller"], Value::Null, "no lease is left");

    let page = persons_page(&rig, &agent, "https://b.example/", true, cx);
    allowed(&agent, page, cx);
    let wait = json!({ "pane": page, "for": "url", "value": "never", "timeoutMs": 20_000 });
    let waiting = send(&agent, None, op("browser_wait", wait), cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    rig.shell.update(cx, |shell, cx| shell.close_pinned(page, cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let closed = taken(&waiting).expect("answered").unwrap_err();
    assert_eq!(closed.code, ErrorCode::BrowserUnavailable, "{}", closed.message);
    assert!(closed.message.contains("closed"), "{}", closed.message);
}

#[gpui_kit::test]
fn a_page_an_extension_closes_ends_consent_as_any_close_does(cx: &mut TestAppContext) {
    let root = repo("control-persons-extension", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-extension", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    for pinned in [true, false] {
        let page = persons_page(&rig, &agent, "https://a.example/", pinned, cx);
        allowed(&agent, page, cx);
        let wait = json!({ "pane": page, "for": "url", "value": "never", "timeoutMs": 20_000 });
        let waiting = send(&agent, None, op("browser_wait", wait), cx);
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert!(taken(&waiting).is_none(), "still waiting");
        let view = read_shell(&rig, cx, |shell, _| shell.workspace.pane(page)?.view().cloned());
        (view.expect("the page")).update(cx, |_, cx| cx.emit(BrowserEvent::ClosedItself));
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        let closed = taken(&waiting).expect("answered").unwrap_err();
        assert_eq!(closed.code, ErrorCode::BrowserUnavailable, "{}", closed.message);
        assert!(closed.message.contains("closed"), "{}", closed.message);
        let open = read_shell(&rig, cx, |shell, _| shell.workspace.pane(page)?.view().cloned());
        assert!(open.is_none(), "pinned {pinned}: the page is closed");
    }
}

#[gpui_kit::test]
fn a_worktrees_page_that_becomes_the_persons_during_a_call_is_theirs(cx: &mut TestAppContext) {
    let root = repo("control-persons-becomes", &[("README.md", "hi\n")]);
    let rig = controlled("control-persons-becomes", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let opened = json!({ "kind": "browser", "url": "https://a.example/" });
    let page = call(&agent, "tab_open", opened, cx).unwrap()["pane"].as_u64().unwrap();
    let held = call(&agent, "browser_claim", json!({ "pane": page }), cx).unwrap();
    assert_eq!(held["held"], json!(true));
    let wait = json!({ "pane": page, "for": "url", "value": "never", "timeoutMs": 20_000 });
    let waiting = send(&agent, None, op("browser_wait", wait), cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    rig.shell.update(cx, |shell, cx| assert!(shell.pin_tab_of(page, usize::MAX, cx)));
    cx.run_until_parked();
    assert_eq!(listed(&agent, page, cx)["controller"], Value::Null);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    let stopped = taken(&waiting).expect("answered").unwrap_err();
    assert_eq!(stopped.code, ErrorCode::ConsentRequired, "{}", stopped.message);
    assert_eq!(reading(&agent, page, cx).code, ErrorCode::ConsentRequired);
}
