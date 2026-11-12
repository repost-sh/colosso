//! Where the accounts' button is drawn in each layout: the Arc sidebar's foot, and Orca's top
//! rail beside the sidebar's toggle.

use gpui_kit::{AnyWindowHandle, Bounds, Entity, Pixels, TestAppContext, VisualTestContext, px};
use koyo_accounts::testing::Sandbox;

use super::kept;
use crate::accounts::TestEnv;
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::shell_with;
use crate::theme;

/// The shell on `sandbox`'s accounts, drawn in `layout`, and where its accounts button is.
pub(super) fn shell_on(
    sandbox: &Sandbox,
    layout: Layout,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Bounds<Pixels>) {
    cx.update(|cx| cx.set_global(TestEnv(sandbox.env())));
    let (window, shell) = shell_with(&[], cx);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = layout));
    cx.run_until_parked();
    let drawn = &mut VisualTestContext::from_window(window, cx);
    drawn.run_until_parked();
    let button = (drawn.debug_bounds("accounts-button")).expect("the button is drawn");
    (window, shell, button)
}

/// The button's bounds in the shell drawn in `layout`, with an account kept.
fn button_in(layout: Layout, cx: &mut TestAppContext) -> Bounds<Pixels> {
    shell_on(&kept(), layout, cx).2
}

#[gpui_kit::test]
fn in_orca_the_button_is_in_the_top_rail_at_its_left(cx: &mut TestAppContext) {
    let button = button_in(Layout::Orca, cx);
    assert!(button.bottom() <= px(theme::TITLE_BAR_HEIGHT), "{button:?}");
    assert!(button.left() < px(200.), "{button:?}");
}

#[gpui_kit::test]
fn in_arc_the_button_is_in_the_sidebars_foot(cx: &mut TestAppContext) {
    let button = button_in(Layout::Arc, cx);
    // The shell's test window is 900 high: the foot is its last row.
    assert!(button.top() > px(800.), "{button:?}");
    assert!(button.left() < px(200.), "{button:?}");
}
