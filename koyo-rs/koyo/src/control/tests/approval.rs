//! Hellos: agents in Koyo's terminals welcomed without asking, external clients refused
//! while external control is off and asked about once per name, the person's answers and
//! silence, and the policy functions the Settings page calls.

use gpui_kit::TestAppContext;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::json;

use super::{
    APPROVAL, DENIAL, activity, allow, call, controlled, external, hello, send_hello, taken,
    terminal,
};
use crate::control::{
    ActivityKind, Control, DEFAULT_PORT, Decision, allow_denied, denied, policy, revoke,
    set_client_scopes, set_enabled, set_port,
};
use crate::test_support::repo;

#[gpui_kit::test]
fn an_agent_in_a_koyo_terminal_is_welcomed_without_asking(cx: &mut TestAppContext) {
    let root = repo("control-hello-terminal", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-terminal", &[&root], cx);
    cx.update(|cx| Control::get_mut(cx).test_terminals.insert("term_1".into()));
    let welcome = hello(&terminal("term_1"), cx).unwrap();
    assert_eq!(welcome.grant, Scope::TERMINAL_AGENT.to_vec());
    assert!(!cx.has_pending_prompt(), "nobody was asked");
    // A terminal Koyo doesn't run doesn't pass for one, however the client names it.
    let refused = hello(&terminal("term_gone"), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::TerminalNotFound);
}

#[gpui_kit::test]
fn an_external_client_is_refused_while_external_control_is_off(cx: &mut TestAppContext) {
    let root = repo("control-hello-off", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-off", &[&root], cx);
    let refused = hello(&external("Claude Code"), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ControlDisabled);
    assert!(!cx.has_pending_prompt());
    cx.update(|cx| set_enabled(true, cx));
    let refused = hello(&external("terminal:term_1"), cx).unwrap_err();
    assert_eq!(
        refused.code,
        ErrorCode::InvalidInput,
        "names of terminals aren't for external clients"
    );
}

#[gpui_kit::test]
fn the_person_allows_a_client_once_for_every_hello_waiting(cx: &mut TestAppContext) {
    let root = repo("control-hello-allow", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-allow", &[&root], cx);
    cx.update(|cx| set_enabled(true, cx));
    let first = send_hello(&external("Claude Code"), cx);
    let second = send_hello(&external("Claude Code"), cx);
    cx.run_until_parked();
    let (question, _) = cx.pending_prompt().expect("the person is asked");
    assert_eq!(question, "Allow “Claude Code” to control Koyo?");
    assert!(taken(&first).is_none(), "the hello waits for the person");
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "one prompt asked for both hellos");
    for answered in [first, second] {
        assert_eq!(
            taken(&answered).unwrap().unwrap().grant,
            Scope::DEFAULT_GRANT.to_vec()
        );
    }
    let allowed = cx
        .read(policy)
        .client("Claude Code")
        .cloned()
        .expect("the client is allowed");
    assert_eq!(allowed.scopes, Scope::DEFAULT_GRANT.to_vec());
    assert!(allowed.last_used.is_some());
    // Allowed, it's welcomed without asking again.
    assert!(hello(&external("Claude Code"), cx).is_ok());
    assert!(!cx.has_pending_prompt());
    let decisions: Vec<_> = activity(cx)
        .into_iter()
        .map(|entry| (entry.client, entry.kind))
        .collect();
    assert_eq!(
        decisions,
        vec![(
            "Claude Code".to_owned(),
            ActivityKind::Approval(Decision::Allowed)
        )]
    );
}

#[gpui_kit::test]
fn a_denied_client_is_refused_without_asking_until_the_denial_passes(cx: &mut TestAppContext) {
    let root = repo("control-hello-deny", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-deny", &[&root], cx);
    cx.update(|cx| set_enabled(true, cx));
    let asked = send_hello(&external("Cursor"), cx);
    cx.run_until_parked();
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    assert_eq!(
        taken(&asked).unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
    assert_eq!(cx.read(denied), vec!["Cursor".to_owned()]);
    let again = hello(&external("Cursor"), cx).unwrap_err();
    assert_eq!(again.code, ErrorCode::UserDeclined);
    assert!(!cx.has_pending_prompt(), "a denial is remembered");
    cx.executor().advance_clock(DENIAL);
    cx.run_until_parked();
    assert!(cx.read(denied).is_empty());
    let asked = send_hello(&external("Cursor"), cx);
    cx.run_until_parked();
    assert!(
        cx.has_pending_prompt(),
        "once the denial passes, the person is asked again"
    );
    cx.simulate_prompt_answer("Deny");
    cx.run_until_parked();
    assert!(taken(&asked).unwrap().is_err());
    // The person changes their mind in Settings.
    cx.update(|cx| allow_denied("Cursor", cx));
    assert!(hello(&external("Cursor"), cx).is_ok());
    assert!(cx.read(denied).is_empty());
}

#[gpui_kit::test]
fn a_client_the_person_doesnt_answer_about_is_declined_and_asked_again(cx: &mut TestAppContext) {
    let root = repo("control-hello-timeout", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-timeout", &[&root], cx);
    cx.update(|cx| set_enabled(true, cx));
    let asked = send_hello(&external("Zed"), cx);
    cx.run_until_parked();
    cx.executor().advance_clock(APPROVAL);
    cx.run_until_parked();
    assert_eq!(
        taken(&asked).unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
    // An answer past the time changes nothing.
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert!(cx.read(policy).client("Zed").is_none());
    assert!(cx.read(denied).is_empty(), "silence isn't a denial");
    let kinds: Vec<_> = activity(cx).into_iter().map(|entry| entry.kind).collect();
    assert_eq!(kinds, vec![ActivityKind::Approval(Decision::TimedOut)]);
    let _again = send_hello(&external("Zed"), cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "the next hello asks again");
}

#[gpui_kit::test]
fn turning_external_control_off_refuses_the_hellos_waiting(cx: &mut TestAppContext) {
    let root = repo("control-hello-disable", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-disable", &[&root], cx);
    cx.update(|cx| set_enabled(true, cx));
    let asked = send_hello(&external("Claude Code"), cx);
    cx.run_until_parked();
    cx.update(|cx| set_enabled(false, cx));
    cx.run_until_parked();
    assert_eq!(
        taken(&asked).unwrap().unwrap_err().code,
        ErrorCode::ControlDisabled
    );
}

#[gpui_kit::test]
fn the_settings_page_sets_what_a_client_may_do(cx: &mut TestAppContext) {
    let root = repo("control-hello-scopes", &[("README.md", "hi\n")]);
    let _rig = controlled("control-hello-scopes", &[&root], cx);
    allow("Claude Code", &Scope::DEFAULT_GRANT, cx);
    let caller = external("Claude Code");
    assert!(call(&caller, "project_select", json!({ "project": root }), cx).is_ok());
    cx.update(|cx| set_client_scopes("Claude Code", vec![Scope::Read, Scope::Read], cx));
    assert_eq!(
        cx.read(policy).client("Claude Code").unwrap().scopes,
        vec![Scope::Read]
    );
    let refused = call(&caller, "project_select", json!({ "project": root }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied);
    cx.update(|cx| revoke("Claude Code", cx));
    let refused = call(&caller, "app_status", json!({}), cx).unwrap_err();
    assert_eq!(
        refused.code,
        ErrorCode::UserDeclined,
        "a revoked client is asked about again"
    );
    assert_eq!(cx.read(policy).port, DEFAULT_PORT);
    cx.update(|cx| set_port(7000, cx));
    assert_eq!(cx.read(policy).port, 7000);
}
