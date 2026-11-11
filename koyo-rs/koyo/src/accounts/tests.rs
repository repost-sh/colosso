//! The accounts as the app shows them and does what the person asks, on a sandbox: the
//! Keychain is the fake `security`, the home a folder of the test's, and Claude's endpoints
//! answer what the test queued. Accounts are kept as the person keeps them: signed in with
//! Claude Code, then added through the switcher.

use gpui_kit::{Context, IntoElement, Render, TestAppContext, VisualTestContext, div, px, size};
use koyo_accounts::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE};
use koyo_accounts::switcher::Switcher;
use koyo_accounts::testing::Sandbox;
use koyo_agent::protocol::ids::Time;
use koyo_agent::protocol::limits::{AccountLimits, LimitStatus, LimitWindow};
use serde_json::json;

use super::model::Problem;
use super::service::TestEnv;
use super::*;
use crate::test_support::in_window;

pub(crate) const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";

/// Signs `sandbox`'s Mac in to Claude Code as `email`, its token good for a day.
fn sign_in(sandbox: &Sandbox, email: &str) {
    let expires = (now() + 86_400.) * 1000.;
    let credential = json!({"claudeAiOauth": {
        "accessToken": format!("sk-ant-oat01-{email}"),
        "refreshToken": format!("sk-ant-ort01-{email}"),
        "expiresAt": expires, "scopes": ["user:inference", "user:profile"],
    }});
    sandbox.security.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        "tester",
        &credential.to_string(),
    );
    let config = json!({"oauthAccount": {
        "emailAddress": email, "organizationName": "Acme",
        "organizationUuid": "org-acme", "accountUuid": format!("uuid-{email}"),
    }});
    std::fs::write(
        paths::global_config_path(&sandbox.env()),
        config.to_string(),
    )
    .unwrap();
}

/// A sandbox signed in as ana@example.com, the login not kept yet.
fn signed_in() -> Sandbox {
    let sandbox = Sandbox::new();
    // The app's clock is the Mac's: the crate's is too, here.
    sandbox.clock.set(now());
    sign_in(&sandbox, "ana@example.com");
    sandbox
}

/// A sandbox signed in as ana@example.com, whose login Koyo keeps as account 1.
pub(crate) fn kept() -> Sandbox {
    let sandbox = signed_in();
    Switcher::new(sandbox.env()).add_account().unwrap();
    sandbox
}

/// A sandbox where Koyo keeps ana@example.com (1) and bob@example.com (2), ana's the Mac's.
fn two_kept() -> Sandbox {
    let sandbox = kept();
    sign_in(&sandbox, "bob@example.com");
    let switcher = Switcher::new(sandbox.env());
    switcher.add_account().unwrap();
    switcher.switch_to("1").unwrap();
    sandbox
}

/// The accounts, on `sandbox`, read once as the cadence's first look reads them.
fn on(sandbox: &Sandbox, cx: &mut TestAppContext) -> Entity<Accounts> {
    let env = sandbox.env();
    cx.update(|cx| {
        cx.set_global(TestEnv(env));
        crate::init(cx);
    });
    cx.run_until_parked();
    cx.update(|cx| accounts(cx))
}

/// Reads the active account again, whatever the cadence says.
fn read(accounts: &Entity<Accounts>, cx: &mut TestAppContext) {
    accounts.update(cx, |accounts, cx| {
        let read = Job::Read {
            active: true,
            other: None,
        };
        accounts.ask(read, cx);
    });
    cx.run_until_parked();
}

fn slot(number: &str, email: &str) -> Slot {
    Slot {
        number: number.to_owned(),
        email: email.to_owned().into(),
    }
}

fn usage(week: f64) -> serde_json::Value {
    json!({
        "five_hour": {"utilization": 12.0, "resets_at": "2099-01-01T00:00:00Z"},
        "seven_day": {"utilization": week, "resets_at": "2099-01-05T00:00:00Z"},
        "limits": [{"kind": "weekly_scoped", "scope": {"model": {"display_name": "Fable"}},
                    "percent": 70.0, "resets_at": "2099-01-05T00:00:00Z"}],
    })
}

fn bars(row: &Row) -> Vec<(&str, Option<f64>)> {
    (row.bars.iter())
        .map(|bar| (bar.label.as_ref(), bar.percent))
        .collect()
}

#[gpui_kit::test]
fn a_login_koyo_doesnt_keep_is_offered_and_added_then_read(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    let accounts = on(&sandbox, cx);
    accounts.read_with(cx, |accounts, _| {
        assert!(accounts.view.rows.is_empty());
        assert_eq!(accounts.view.unmanaged.as_deref(), Some("ana@example.com"));
    });
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    accounts.update(cx, |accounts, cx| accounts.ask(Job::Add, cx));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(accounts.view.unmanaged, None);
        let row = accounts.view.active().unwrap();
        assert_eq!(
            (row.number.as_str(), row.email.as_ref()),
            ("1", "ana@example.com")
        );
        assert_eq!(row.organisation.as_deref(), Some("Acme"));
        assert_eq!(bars(row), [("7 days", Some(41.)), ("Fable", Some(70.))]);
    });
}

#[gpui_kit::test]
fn the_active_accounts_week_and_models_show_once_read(cx: &mut TestAppContext) {
    let sandbox = kept();
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    let accounts = on(&sandbox, cx);
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(bars(row), [("7 days", Some(41.)), ("Fable", Some(70.))]);
        assert!(row.measured_at.is_some() && row.problem.is_none());
    });
    // Read with the token Claude Code has; never a refresh.
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 1);
    assert!(
        sandbox
            .http
            .requests_to("https://platform.claude.com")
            .is_empty()
    );
}

#[gpui_kit::test]
fn a_failed_reading_keeps_the_last_bars_and_backs_off(cx: &mut TestAppContext) {
    let sandbox = kept();
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    let accounts = on(&sandbox, cx);
    sandbox.http.queue(
        USAGE,
        Ok(koyo_accounts::http::Response {
            status: 429,
            headers: vec![("retry-after".into(), "3600".into())],
            body: b"{}".to_vec(),
        }),
    );
    // Past the store's freshness floor.
    sandbox.clock.advance(600.);
    read(&accounts, cx);
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(row.bars[0].percent, Some(41.));
        assert!(
            matches!(row.problem, Some(Problem::RateLimited { .. })),
            "{:?}",
            row.problem
        );
    });
    // Inside the backoff, nothing is asked.
    read(&accounts, cx);
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 2);
}

fn window(utilization: f64) -> LimitWindow {
    LimitWindow {
        utilization,
        resets_at: Time("2099-01-05T00:00:00Z".into()),
        status: None,
    }
}

#[gpui_kit::test]
fn a_chats_limits_move_the_week_and_the_model_at_once(cx: &mut TestAppContext) {
    let sandbox = kept();
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    let accounts = on(&sandbox, cx);
    let limits = AccountLimits {
        status: LimitStatus::Allowed,
        window: None,
        utilization: None,
        resets_at: None,
        five_hour: Some(window(0.2)),
        seven_day: Some(window(0.4567)),
        seven_day_overage_included: Some(window(0.9)),
    };
    // A reply after the reading: the store keeps whichever is newer.
    sandbox.clock.advance(5.);
    cx.update(|cx| limits_heard(&limits, cx));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(bars(row), [("7 days", Some(45.67)), ("Fable", Some(90.))]);
    });
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 1, "for free");
}

#[gpui_kit::test]
fn a_switch_makes_the_other_account_the_macs_login_and_reads_it(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    let accounts = on(&sandbox, cx);
    sandbox.http.queue_json(USAGE, 200, &usage(7.));
    accounts.update(cx, |accounts, cx| {
        accounts.ask(Job::Switch(slot("2", "bob@example.com")), cx);
    });
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(row.email.as_ref(), "bob@example.com");
        assert_eq!(row.bars[0].percent, Some(7.));
        assert_eq!(
            (accounts.view.doing.as_ref(), accounts.view.said.as_ref()),
            (None, None)
        );
    });
    let live = sandbox.security.get(CLAUDE_CODE_KEYCHAIN_SERVICE, "tester");
    assert!(live.unwrap().contains("sk-ant-oat01-bob@example.com"));
}

/// Opens the panel over a blank window.
fn open_panel(accounts: &Entity<Accounts>, cx: &mut TestAppContext) -> gpui_kit::AnyWindowHandle {
    let (parent, _) = in_window(size(px(400.), px(300.)), cx, |_, cx| cx.new(|_| Blank));
    parent
        .update(cx, |_, window, cx| {
            accounts.update(cx, |accounts, cx| accounts.toggle_panel(window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    parent
}

#[gpui_kit::test]
fn the_panel_reads_another_account_and_skips_one_backing_off(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    sign_in(&sandbox, "carl@example.com");
    let switcher = Switcher::new(sandbox.env());
    switcher.add_account().unwrap();
    switcher.switch_to("1").unwrap();
    sandbox.http.queue_json(USAGE, 200, &usage(41.));
    let accounts = on(&sandbox, cx);
    // Opening it reads the first of the others, which is asked to wait.
    let limited = koyo_accounts::http::Response {
        status: 429,
        headers: vec![("retry-after".into(), "3600".into())],
        body: b"{}".to_vec(),
    };
    sandbox.http.queue(USAGE, Ok(limited));
    open_panel(&accounts, cx);
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 2);
    // A minute on, the one backing off waits its turn and the next is read.
    sandbox.http.queue_json(USAGE, 200, &usage(7.));
    accounts.update(cx, |accounts, cx| {
        accounts.running.as_mut().unwrap().cadence = poll::Cadence::default();
        accounts.look(cx);
    });
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        let row = |email: &str| {
            (accounts.view.rows.iter())
                .find(|row| row.email.as_ref() == email)
                .unwrap()
        };
        assert!(matches!(
            row("bob@example.com").problem,
            Some(Problem::RateLimited { .. })
        ));
        assert_eq!(row("carl@example.com").bars[0].percent, Some(7.));
    });
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 3);
}

#[gpui_kit::test]
fn holding_out_and_removing_change_the_list(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    let accounts = on(&sandbox, cx);
    accounts.update(cx, |accounts, cx| {
        accounts.ask(Job::Hold(slot("2", "bob@example.com"), true), cx);
    });
    cx.run_until_parked();
    let disabled = |accounts: &Accounts| {
        (accounts.view.rows.iter())
            .map(|row| (row.email.to_string(), row.disabled))
            .collect::<Vec<_>>()
    };
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(
            disabled(accounts),
            [
                ("ana@example.com".into(), false),
                ("bob@example.com".into(), true)
            ]
        );
    });
    accounts.update(cx, |accounts, cx| {
        accounts.ask(Job::Remove(slot("2", "bob@example.com")), cx);
    });
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(disabled(accounts), [("ana@example.com".into(), false)]);
    });
    // The Mac's login stays as it was.
    let live = sandbox.security.get(CLAUDE_CODE_KEYCHAIN_SERVICE, "tester");
    assert!(live.unwrap().contains("sk-ant-oat01-ana@example.com"));
}

#[gpui_kit::test]
fn a_switch_that_fails_says_why(cx: &mut TestAppContext) {
    let sandbox = kept();
    let accounts = on(&sandbox, cx);
    accounts.update(cx, |accounts, cx| {
        accounts.ask(Job::Switch(slot("7", "nobody@example.com")), cx);
    });
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        let said = accounts.view.said.as_deref().unwrap();
        assert!(
            said.starts_with("Couldn't switch to nobody@example.com"),
            "{said}"
        );
        assert_eq!(
            accounts.view.active().unwrap().email.as_ref(),
            "ana@example.com"
        );
    });
}

/// What the window the panel opens from shows: nothing.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui_kit::test]
fn the_panel_opens_over_the_button_and_a_click_switches(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    let accounts = on(&sandbox, cx);
    let parent = open_panel(&accounts, cx);
    let menu = cx
        .windows()
        .into_iter()
        .find(|each| *each != parent)
        .unwrap();
    let cx = &mut VisualTestContext::from_window(menu, cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("accounts-row-ana@example.com").is_some());
    let bob = cx.debug_bounds("accounts-row-bob@example.com").unwrap();
    cx.simulate_click(bob.center(), gpui_kit::Modifiers::none());
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(
            accounts.view.active().unwrap().email.as_ref(),
            "bob@example.com"
        );
    });
}

#[gpui_kit::test]
fn off_nothing_runs_and_the_button_is_empty(cx: &mut TestAppContext) {
    let sandbox = kept();
    let accounts = on(&sandbox, cx);
    let asked = sandbox.http.requests().len();
    cx.update(|cx| settings::update(cx, |settings| settings.accounts.manage = false));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert!(accounts.running.is_none());
        assert!(accounts.view.rows.is_empty());
    });
    accounts.update(cx, Accounts::look);
    cx.run_until_parked();
    assert_eq!(sandbox.http.requests().len(), asked);
}

#[gpui_kit::test]
fn auto_switch_over_the_line_moves_the_macs_login_and_says_so(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    // The first look reads ana over the line; the tick reads bob, well under it.
    sandbox.http.queue_json(USAGE, 200, &usage(95.));
    sandbox.http.queue_json(USAGE, 200, &usage(10.));
    let accounts = on(&sandbox, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.accounts.auto_switch = true));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(
            accounts.view.active().unwrap().email.as_ref(),
            "bob@example.com"
        );
        let auto = accounts.view.auto_switch.as_ref().unwrap();
        let said = auto.said.as_deref().unwrap();
        assert!(
            said.starts_with("ana@example.com reached the threshold: moved to bob@example.com"),
            "{said}"
        );
    });
    let live = sandbox.security.get(CLAUDE_CODE_KEYCHAIN_SERVICE, "tester");
    assert!(live.unwrap().contains("sk-ant-oat01-bob@example.com"));
    // Its token was good for a day: nothing was refreshed to land.
    assert!(
        sandbox
            .http
            .requests_to("https://platform.claude.com")
            .is_empty()
    );
}

#[gpui_kit::test]
fn turning_auto_switch_off_drops_the_tick_waiting(cx: &mut TestAppContext) {
    let sandbox = two_kept();
    // Bob is readable and well under the line: a tick that ran would switch to him.
    sandbox.http.queue_json(USAGE, 200, &usage(95.));
    sandbox.http.queue_json(USAGE, 200, &usage(10.));
    let accounts = on(&sandbox, cx);
    // A reading on the line, so the tick that turning it on asks for waits behind it.
    accounts.update(cx, |accounts, cx| {
        let read = Job::Read {
            active: true,
            other: None,
        };
        accounts.ask(read, cx);
    });
    cx.update(|cx| settings::update(cx, |settings| settings.accounts.auto_switch = true));
    cx.update(|cx| settings::update(cx, |settings| settings.accounts.auto_switch = false));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert_eq!(
            accounts.view.active().unwrap().email.as_ref(),
            "ana@example.com"
        );
        assert_eq!(accounts.view.auto_switch, None);
    });
    assert_eq!(sandbox.http.requests_to(USAGE).len(), 1);
}
