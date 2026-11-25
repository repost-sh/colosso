//! The panel's button beside a login Koyo doesn't keep: it leads to Settings → Accounts, where
//! the login is added and the rest is set, rather than adding it from the panel.

use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};

use super::layout::shell_on;
use super::signed_in;
use crate::accounts::accounts;
use crate::popup::{self, Kind};
use crate::model::SettingsTab;
use crate::navigation::SettingsPage;

#[gpui_kit::test]
fn the_button_beside_the_macs_login_opens_settings_on_accounts(cx: &mut TestAppContext) {
    let sandbox = signed_in();
    let (window, shell, button) = shell_on(&sandbox, cx);
    let shown = &mut VisualTestContext::from_window(window, cx);
    shown.simulate_click(button.center(), Modifiers::none());
    shown.run_until_parked();
    let panel = cx
        .read(|cx| popup::window(Kind::Menu, cx))
        .expect("the panel is up");
    let up = &mut VisualTestContext::from_window(panel, cx);
    let to_settings = (up.debug_bounds("accounts-to-settings")).expect("the button is drawn");
    up.simulate_click(to_settings.center(), Modifiers::none());
    up.run_until_parked();
    let page = shell.read_with(cx, |shell, _| shell.workspace.settings().map(SettingsTab::page));
    assert_eq!(page, Some(SettingsPage::Accounts));
    assert!(!cx.windows().contains(&panel), "the panel went");
    let unmanaged = cx.read(|cx| accounts(cx).read(cx).view.unmanaged.clone());
    assert_eq!(unmanaged.as_deref(), Some("ana@example.com"));
}
