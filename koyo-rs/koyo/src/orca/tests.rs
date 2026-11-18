//! The Orca sidebar in a test window.

use gpui_kit::{AnyWindowHandle, TestAppContext, VisualTestContext};

use crate::settings::Layout;
use crate::test_support::{click, draw, shell_on};

/// Whether the sidebar lists the first project's checkout.
fn checkout_listed(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.debug_bounds("orca-branch-0-0").is_some()
}

/// What's typed in the sidebar's search field narrows the worktrees listed to those it names,
/// and Escape, clearing it, lists them all again.
#[gpui_kit::test]
fn typing_in_the_search_field_narrows_the_worktrees_listed(cx: &mut TestAppContext) {
    let (window, _, _) = shell_on("orca-search", Layout::Orca, cx);
    assert!(checkout_listed(window, cx), "the checkout is listed");

    click(window, "sidebar-search", cx);
    cx.simulate_input(window, "nothing");
    assert!(
        !checkout_listed(window, cx),
        "a search that names no worktree lists none"
    );

    cx.simulate_keystrokes(window, "escape");
    assert!(
        checkout_listed(window, cx),
        "the search cleared lists the checkout again"
    );

    cx.simulate_input(window, "mai");
    assert!(
        checkout_listed(window, cx),
        "a search that names the checkout lists it"
    );
}
