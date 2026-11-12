//! The accounts by provider, each group under its mark, and the panel's window fitting what's
//! drawn as accounts come and go and notes show: the panel on made-up views, Koyo off the
//! sandbox's login so nothing else changes them.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext, px, size,
};
use koyo_accounts::testing::Sandbox;

use super::on;
use crate::accounts::Accounts;
use crate::accounts::model::{AutoSwitch, Group, Provider, Row, View};
use crate::accounts::panel::{AccountsPanel, WIDTH};
use crate::menu::MenuBody as _;
use crate::settings;
use crate::test_support::in_window;

fn row(number: &str, email: &str) -> Row {
    Row {
        number: number.to_owned(),
        provider: Provider::Anthropic,
        email: email.to_owned().into(),
        organisation: None,
        plan: Some("Max 20x".into()),
        active: number == "1",
        disabled: false,
        switchable: true,
        bars: Vec::new(),
        measured_at: None,
        problem: None,
    }
}

#[test]
fn claudes_accounts_and_the_macs_login_koyo_doesnt_keep_are_one_group() {
    let (ana, bob) = (row("1", "ana@example.com"), row("2", "bob@example.com"));
    let mut view = View {
        rows: vec![ana.clone(), bob.clone()],
        unmanaged: Some("dee@example.com".into()),
        ..View::default()
    };
    let dee = view.unmanaged.clone();
    let group = Group {
        provider: Provider::Anthropic,
        rows: vec![&ana, &bob],
        unmanaged: dee.as_ref(),
    };
    assert_eq!(view.groups(), [group]);
    view.rows.clear();
    let alone = Group {
        provider: Provider::Anthropic,
        rows: Vec::new(),
        unmanaged: dee.as_ref(),
    };
    assert_eq!(view.groups(), [alone]);
    assert!(View::default().groups().is_empty());
}

/// The panel on made-up accounts, in a window as wide as it is.
struct Shown {
    accounts: Entity<Accounts>,
    window: AnyWindowHandle,
    panel: Entity<AccountsPanel>,
}

impl Shown {
    /// Koyo off the sandbox's login, so only what's shown changes the accounts.
    fn new(cx: &mut TestAppContext) -> Shown {
        let accounts = on(&Sandbox::new(), cx);
        cx.update(|cx| settings::update(cx, |settings| settings.accounts.manage = false));
        cx.run_until_parked();
        let (window, panel) = in_window(size(px(WIDTH), px(900.)), cx, |_, cx| {
            cx.new(|cx| AccountsPanel::new(&accounts, View::default(), cx))
        });
        Shown {
            accounts,
            window,
            panel,
        }
    }

    /// Shows `view` and checks the panel is drawn as tall as it says it is, which its window
    /// is sized by.
    fn fits(&self, view: View, cx: &mut TestAppContext) -> VisualTestContext {
        self.accounts.update(cx, |accounts, cx| {
            accounts.view = view;
            cx.notify();
        });
        let mut drawn = VisualTestContext::from_window(self.window, cx);
        drawn.run_until_parked();
        let end = (drawn.debug_bounds("accounts-panel-end")).expect("the panel is drawn");
        let extent = drawn.update(|window, cx| self.panel.read(cx).extent(window, cx));
        assert!(
            (end.top() - extent.height).abs() < px(0.5),
            "drawn {:?} tall, sized {:?}",
            end.top(),
            extent.height
        );
        drawn
    }
}

#[gpui_kit::test]
fn the_mark_heads_claudes_accounts_and_the_window_fits_as_they_change(cx: &mut TestAppContext) {
    let shown = Shown::new(cx);
    let mark = |drawn: &mut VisualTestContext| drawn.debug_bounds("accounts-mark-Anthropic");
    let one = View {
        rows: vec![row("1", "ana@example.com")],
        ..View::default()
    };
    let drawn = &mut shown.fits(one, cx);
    let above = mark(drawn).expect("the mark is drawn");
    let ana = (drawn.debug_bounds("accounts-row-ana@example.com")).expect("ana is drawn");
    assert!(above.bottom() <= ana.top(), "{above:?} over {ana:?}");
    assert!(above.size.height > px(0.), "{above:?}");
    // Another account, a note that wraps and what auto-switch did.
    let said = "Couldn't switch to bob@example.com: Claude Code's lock stayed held; try again.";
    let more = View {
        rows: vec![row("1", "ana@example.com"), row("2", "bob@example.com")],
        said: Some(said.to_owned()),
        auto_switch: Some(AutoSwitch {
            threshold: 90.,
            said: Some("bob@example.com reached the threshold: moved to ana@example.com.".into()),
        }),
        ..View::default()
    };
    shown.fits(more, cx);
    // Down to the Mac's login Koyo doesn't keep: still Claude's, under the mark.
    let unkept = View {
        unmanaged: Some("dee@example.com".into()),
        said: Some(said.to_owned()),
        ..View::default()
    };
    let drawn = &mut shown.fits(unkept, cx);
    assert!(mark(drawn).is_some());
    // Nothing to group: no mark.
    let drawn = &mut shown.fits(View::default(), cx);
    assert_eq!(mark(drawn), None);
}
