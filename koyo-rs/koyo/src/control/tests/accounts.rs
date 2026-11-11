//! `claude_accounts_list` as agents read it, through the control pipeline: the Mac's login on a
//! sandbox (the fake security, a sandboxed `~/.claude.json`, a queued usage answer), its week,
//! and what keeps a reading from being taken as a stable kind with its remedy.

use gpui_kit::TestAppContext;
use koyo_accounts::http::Response;
use serde_json::{Value, json};

use super::{call, controlled, terminal};
use crate::accounts::tests::{USAGE, signed_in};
use crate::accounts::{self, TestEnv};
use crate::test_support::repo;

/// `answer` without its accounts' `readAt`, which is when the test ran.
fn timeless(mut answer: Value) -> Value {
    for account in answer["accounts"].as_array_mut().into_iter().flatten() {
        let read = account
            .as_object_mut()
            .and_then(|account| account.remove("readAt"));
        assert!(read.is_none_or(|read| read.is_u64()));
    }
    answer
}

#[gpui_kit::test]
fn agents_list_the_macs_login_with_its_week(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    sandbox.http.queue_json(
        USAGE,
        200,
        &json!({"seven_day": {"utilization": 41.0, "resets_at": "2099-01-05T00:00:00Z"}}),
    );
    let env = sandbox.env();
    cx.update(|cx| cx.set_global(TestEnv(env)));
    let root = repo("control-accounts", &[("README.md", "hi\n")]);
    let _rig = controlled("control-accounts", &[&root], cx);
    // Koyo coming to the front is the cadence's first look.
    cx.update(accounts::window_activated);
    cx.run_until_parked();
    let listed = call(&terminal("term_1"), "claude_accounts_list", json!({}), cx).unwrap();
    // 2099-01-05T00:00:00Z.
    let resets = 4_071_254_400_000_u64;
    assert_eq!(
        timeless(listed),
        json!({"managed": true, "accounts": [{
            "email": "ana@example.com", "organisation": "Acme", "active": true,
            "windows": [{ "name": "7 days", "percent": 41.0, "resetsAt": resets, "aheadOfPace": false }],
        }]})
    );
}

#[gpui_kit::test]
fn a_rate_limit_is_listed_as_its_kind_with_when_it_ends(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    let limited = Response {
        status: 429,
        headers: vec![("retry-after".into(), "120".into())],
        body: b"{}".to_vec(),
    };
    sandbox.http.queue(USAGE, Ok(limited));
    let env = sandbox.env();
    cx.update(|cx| cx.set_global(TestEnv(env)));
    let root = repo("control-accounts-limited", &[("README.md", "hi\n")]);
    let _rig = controlled("control-accounts-limited", &[&root], cx);
    cx.update(accounts::window_activated);
    cx.run_until_parked();
    let listed = call(&terminal("term_1"), "claude_accounts_list", json!({}), cx).unwrap();
    let problem = &listed["accounts"][0]["problem"];
    assert_eq!(problem["kind"], json!("rate_limited"));
    assert!(
        problem["message"]
            .as_str()
            .unwrap()
            .starts_with("Rate-limited until ")
    );
    assert!(problem["until"].is_u64());
}
