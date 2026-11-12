//! The accounts' panel and their button as the app draws them, for accounts made up to show each
//! state: the active one near auto-switch's line with a model's window, a team's at the line,
//! one rate-limited on an old reading, auto-switch on with what it last did; and a Mac's login
//! Koyo doesn't keep yet, with what went wrong. Saved as PNGs in `.superpowers/qa/accounts/`,
//! dark and light. The panel is on a menu's tint and the button on a strip like the sidebar's
//! foot; the glass's blur is the menu window's, which an off-screen window doesn't have. The
//! check's own settings keep Koyo off the Mac's login, which it never reads.
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/accounts
//! KOYO_CONFIG_DIR=$Q/config KOYO_DATA_DIR=$Q/data KOYO_LOG_DIR=$Q/logs \
//!   TMPDIR=$PWD/.superpowers/q \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-accounts
//! ```

use std::process::ExitCode;
use std::sync::Arc;

use gpui_kit::base::Root;
use gpui_kit::{
    Action, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    VisualTestAppContext, Window, div, hsla, px, size,
};

use koyo_accounts::pace::WEEKLY_PERIOD_S;

use super::model::{AutoSwitch, Bar, Problem, Provider, Row, View, WEEKLY};
use super::panel::AccountsPanel;
use super::{Accounts, button};
use crate::menu::MenuBody as _;
use crate::menu::PADDING;
use crate::theme::{self, UseDarkTheme, UseLightTheme};
use crate::visual::{Result, out_dir};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/qa/accounts";
const DAY: f64 = 86_400.;

/// Draws the pictures and says where they are, or why it couldn't.
pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    let out = out_dir(OUT)?;
    // Koyo off the Mac's login before anything starts, so only the made-up accounts are drawn
    // and the real login is never read.
    let config = std::env::var_os("KOYO_CONFIG_DIR").ok_or("set KOYO_CONFIG_DIR")?;
    let settings = std::path::Path::new(&config).join("settings.json");
    std::fs::write(&settings, r#"{"accounts": {"manage": false}}"#)
        .map_err(|error| error.to_string())?;
    let platform = gpui_kit::platform::current_platform(false);
    let mut cx =
        VisualTestAppContext::with_asset_source(platform, Arc::new(gpui_kit::assets::AllAssets));
    cx.update(crate::init);
    let accounts = cx.update(|cx| super::accounts(cx));
    let now = super::now();
    let mut said = Vec::new();
    for (name, view) in [("accounts", kept(now)), ("unkept", unkept())] {
        accounts.update(&mut cx, |accounts, cx| {
            accounts.view = view.clone();
            cx.notify();
        });
        let window = cx
            .open_offscreen_window(size(px(380.), px(520.)), |window, cx| {
                let stage = cx.new(|cx| Stage::new(&accounts, view, cx));
                cx.new(|cx| Root::new(stage, window, cx))
            })
            .map_err(|error| error.to_string())?;
        let themes: [(&dyn Action, &str); 2] = [(&UseDarkTheme, "dark"), (&UseLightTheme, "light")];
        for (theme, shade) in themes {
            cx.update(|cx| cx.dispatch_action(theme));
            cx.run_until_parked();
            (cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx)))
                .map_err(|error| error.to_string())?;
            let image =
                (cx.capture_screenshot(window.into())).map_err(|error| error.to_string())?;
            let path = out.join(format!("{name}-{shade}.png"));
            image.save(&path).map_err(|error| error.to_string())?;
            said.push(format!("saved {}", path.display()));
        }
    }
    Ok(said)
}

/// The panel over a strip like the sidebar's foot, holding the button.
struct Stage {
    accounts: Entity<Accounts>,
    panel: Entity<AccountsPanel>,
}

impl Stage {
    fn new(accounts: &Entity<Accounts>, view: View, cx: &mut Context<Self>) -> Stage {
        let panel = cx.new(|cx| AccountsPanel::new(accounts, view, cx));
        Stage {
            accounts: accounts.clone(),
            panel,
        }
    }
}

impl Render for Stage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let view = self.accounts.read(cx).view.clone();
        let extent = self.panel.read(cx).extent(window, cx);
        // Something like what's behind the glass: the app's backdrop, a little lighter.
        let dark = p.text.l > 0.5;
        let backdrop = if dark {
            hsla(0.08, 0.2, 0.16, 1.)
        } else {
            hsla(0.1, 0.3, 0.86, 1.)
        };
        let foot = div()
            .w(px(260.))
            .h(px(34.))
            .px(px(6.))
            .flex()
            .items_center()
            .rounded(px(10.))
            .bg(p.menu)
            .child(div().w(px(24.)).child(button::drawn(&view, cx)))
            .child(
                div().flex_1().flex().justify_center().child(
                    div()
                        .w(px(18.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(p.text.opacity(0.9)),
                ),
            )
            .child(div().w(px(24.)).text_color(p.muted).child("+"));
        div()
            .size_full()
            .bg(backdrop)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                // As the menu's window holds it: padded all round, no edge of its own.
                div()
                    .w(extent.width + px(2. * PADDING))
                    .h(extent.height + px(2. * PADDING))
                    .p(px(PADDING))
                    .rounded(px(12.))
                    .bg(p.menu)
                    .child(self.panel.clone()),
            )
            .child(foot)
    }
}

fn window(label: &str, percent: Option<f64>, resets_in: f64, now: f64) -> Bar {
    Bar {
        label: label.to_owned().into(),
        percent,
        resets_at: Some(now + resets_in),
        pace: Some((1. - resets_in / WEEKLY_PERIOD_S) * 100.),
    }
}

fn account(number: &str, email: &str, plan: &str, bars: Vec<Bar>, read_ago: f64, now: f64) -> Row {
    Row {
        number: number.to_owned(),
        provider: Provider::Anthropic,
        email: email.to_owned().into(),
        organisation: None,
        plan: Some(plan.to_owned().into()),
        active: false,
        disabled: false,
        switchable: true,
        bars,
        measured_at: Some(now - read_ago),
        problem: None,
    }
}

/// Three accounts and auto-switch on.
fn kept(now: f64) -> View {
    let reset = 3. * DAY + 11. * 3600.;
    let mut active = account(
        "1",
        "ana@example.com",
        "Ultra",
        vec![
            window(WEEKLY, Some(81.), reset, now),
            window("Fable", Some(69.), reset, now),
        ],
        60.,
        now,
    );
    active.active = true;
    let mut team = account(
        "2",
        "bob@acme.dev",
        "Team",
        vec![
            window(WEEKLY, Some(93.), DAY + 2. * 3600., now),
            window("Fable", Some(40.), DAY + 2. * 3600., now),
        ],
        240.,
        now,
    );
    team.organisation = Some("Acme Inc.".into());
    let mut limited = account(
        "3",
        "carl@example.com",
        "Pro",
        vec![window(WEEKLY, Some(23.), 6. * DAY, now)],
        2. * 3600.,
        now,
    );
    limited.problem = Some(Problem::RateLimited {
        until: now + 40. * 60.,
    });
    View {
        rows: vec![active, team, limited],
        auto_switch: Some(AutoSwitch {
            threshold: 90.,
            said: Some(
                "bob@acme.dev reached the threshold: moved to ana@example.com at 09:12.".into(),
            ),
        }),
        ..View::default()
    }
}

/// A Mac's login Koyo doesn't keep yet, and a failure.
fn unkept() -> View {
    View {
        unmanaged: Some("dee@example.com".into()),
        said: Some(
            "Couldn't add the Mac's login: Claude Code's lock stayed held; try again.".into(),
        ),
        ..View::default()
    }
}
