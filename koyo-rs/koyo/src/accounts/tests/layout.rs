//! Where the accounts' button is drawn: the sidebar's foot (`docs/layout.md`).

use gpui_kit::{AnyWindowHandle, Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};
use koyo_accounts::testing::Sandbox;

use super::kept;
use crate::accounts::TestEnv;
use crate::shell::Shell;
use crate::test_support::shell_with;

/// The shell on `sandbox`'s accounts, drawn, and where its accounts button is.
pub(super) fn shell_on(
    sandbox: &Sandbox,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Bounds<Pixels>) {
    cx.update(|cx| cx.set_global(TestEnv(sandbox.env())));
    let (window, shell) = shell_with(&[], cx);
    cx.run_until_parked();
    let drawn = &mut VisualTestContext::from_window(window, cx);
    drawn.run_until_parked();
    let button = (drawn.debug_bounds("accounts-button")).expect("the button is drawn");
    (window, shell, button)
}

#[gpui_kit::test]
fn the_button_is_in_the_sidebars_foot(cx: &mut TestAppContext) {
    let (window, _, button) = shell_on(&kept(), cx);
    let foot = (VisualTestContext::from_window(window, cx).debug_bounds("sidebar-foot"))
        .expect("the sidebar's foot");
    assert!(foot.contains(&button.center()), "{button:?} in {foot:?}");
    assert!(button.top() > px(800.), "{button:?}");
    assert!(button.left() < px(200.), "{button:?}");
}
