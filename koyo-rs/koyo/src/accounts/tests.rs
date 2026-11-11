//! The accounts as the app shows them, on a sandbox: the Keychain is the fake `security`, the
//! home a folder of the test's, and Claude's usage endpoint answers what the test queued.

use gpui_kit::{Context, IntoElement, Render, TestAppContext, VisualTestContext, div, px, size};
use koyo_accounts::paths::{self, CLAUDE_CODE_KEYCHAIN_SERVICE};
use koyo_accounts::testing::Sandbox;
use koyo_agent::protocol::ids::Time;
use koyo_agent::protocol::limits::{AccountLimits, LimitStatus, LimitWindow};
use serde_json::json;

use super::model::Problem;
use super::service::TestEnv;
use super::*;
use crate::test_support::in_window;

pub(crate) const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";

/// A sandbox signed in as ana@example.com, its token good for a day.
pub(crate) fn signed_in() -> Sandbox {
    let sandbox = Sandbox::new();
    // The app's clock is the Mac's: the crate's is too, here.
    sandbox.clock.set(now());
    let expires = (now() + 86_400.) * 1000.;
    let credential = json!({"claudeAiOauth": {
        "accessToken": "sk-ant-oat01-test", "refreshToken": "sk-ant-ort01-test",
        "expiresAt": expires, "scopes": ["user:inference", "user:profile"],
    }});
    sandbox.security.set(
        CLAUDE_CODE_KEYCHAIN_SERVICE,
        "tester",
        &credential.to_string(),
    );
    let config = json!({"oauthAccount": {
        "emailAddress": "ana@example.com", "organizationName": "Acme",
    }});
    std::fs::write(
        paths::global_config_path(&sandbox.env()),
        config.to_string(),
    )
    .unwrap();
    sandbox
}

/// The accounts, on `sandbox`, read once as the cadence's first look reads them.
fn on(sandbox: &Sandbox, cx: &mut TestAppContext) -> Entity<Accounts> {
    let env = sandbox.env();
    cx.update(|cx| {
        cx.set_global(TestEnv(env));
        crate::init(cx);
    });
    let accounts = cx.update(|cx| accounts(cx));
    accounts.update(cx, Accounts::look);
    cx.run_until_parked();
    accounts
}

/// Reads again, whatever the cadence says.
fn read(accounts: &Entity<Accounts>, cx: &mut TestAppContext) {
    accounts.update(cx, |accounts, cx| {
        if let Some(running) = &mut accounts.running {
            running.service.read(cx);
        }
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_macs_login_and_its_week_show_once_read(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    sandbox.http.queue_json(
        USAGE,
        200,
        &json!({
            "five_hour": {"utilization": 12.0, "resets_at": "2099-01-01T00:00:00Z"},
            "seven_day": {"utilization": 41.0, "resets_at": "2099-01-05T00:00:00Z"},
            "limits": [{"kind": "weekly_scoped", "scope": {"model": {"display_name": "Fable"}},
                        "percent": 70.0, "resets_at": "2099-01-05T00:00:00Z"}],
        }),
    );
    let accounts = on(&sandbox, cx);
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(
            (row.email.as_ref(), row.organisation.as_deref()),
            ("ana@example.com", Some("Acme"))
        );
        let bars: Vec<_> = row
            .bars
            .iter()
            .map(|bar| (bar.label.as_ref(), bar.percent))
            .collect();
        assert_eq!(bars, [("7 days", Some(41.)), ("Fable", Some(70.))]);
        assert!(row.measured_at.is_some() && row.problem.is_none());
    });
    // The token went in the header, on curl's stdin in production; never a refresh.
    assert_eq!(sandbox.http.requests().len(), 1);
}

#[gpui_kit::test]
fn a_failed_reading_keeps_the_last_bars_and_backs_off(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    sandbox
        .http
        .queue_json(USAGE, 200, &json!({"seven_day": {"utilization": 41.0}}));
    let accounts = on(&sandbox, cx);
    sandbox.http.queue(
        USAGE,
        Ok(koyo_accounts::http::Response {
            status: 429,
            headers: vec![("retry-after".into(), "3600".into())],
            body: b"{}".to_vec(),
        }),
    );
    read(&accounts, cx);
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(row.bars[0].percent, Some(41.));
        // 3600 s asked, over 600: cswap's 900 s margin on top.
        let Some(Problem::RateLimited { until }) = row.problem else {
            panic!("{:?}", row.problem);
        };
        assert!((until - now() - 4500.).abs() < 5.);
    });
    // Inside the backoff, nothing is asked.
    read(&accounts, cx);
    assert_eq!(sandbox.http.requests().len(), 2);
}

#[gpui_kit::test]
fn a_locked_keychain_is_still_signed_in(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    sandbox
        .security
        .answer_all(koyo_accounts::testing::Answer::Exit(51));
    let accounts = on(&sandbox, cx);
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        assert_eq!(row.problem, Some(Problem::KeychainUnavailable));
    });
    assert!(sandbox.http.requests().is_empty());
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
    let sandbox = signed_in();
    sandbox
        .http
        .queue_json(USAGE, 200, &json!({"seven_day": {"utilization": 41.0}}));
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
    cx.update(|cx| limits_heard(&limits, cx));
    accounts.read_with(cx, |accounts, _| {
        let row = accounts.view.active().unwrap();
        let bars: Vec<_> = row
            .bars
            .iter()
            .map(|bar| (bar.label.as_ref(), bar.percent))
            .collect();
        assert_eq!(bars, [("7 days", Some(45.67)), ("Fable", Some(90.))]);
    });
    assert_eq!(sandbox.http.requests().len(), 1, "for free");
}

/// What the window the panel opens from shows: nothing.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui_kit::test]
fn the_panel_opens_over_the_button_with_a_row_for_the_login(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    sandbox
        .http
        .queue_json(USAGE, 200, &json!({"seven_day": {"utilization": 41.0}}));
    let accounts = on(&sandbox, cx);
    let (parent, _) = in_window(size(px(400.), px(300.)), cx, |_, cx| cx.new(|_| Blank));
    parent
        .update(cx, |_, window, cx| {
            accounts.update(cx, |accounts, cx| accounts.toggle_panel(window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    let menu = cx
        .windows()
        .into_iter()
        .find(|each| *each != parent)
        .unwrap();
    let cx = &mut VisualTestContext::from_window(menu, cx);
    cx.run_until_parked();
    assert!(cx.debug_bounds("accounts-row-ana@example.com").is_some());
}

#[gpui_kit::test]
fn off_nothing_runs_and_the_button_is_empty(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    let accounts = on(&sandbox, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.accounts.manage = false));
    cx.run_until_parked();
    accounts.read_with(cx, |accounts, _| {
        assert!(accounts.running.is_none());
        assert!(accounts.view.rows.is_empty());
    });
    // Nothing more was asked than the first reading, which found nothing queued.
    assert_eq!(sandbox.http.requests().len(), 1);
}
