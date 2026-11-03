use gpui_kit::{
    AnyWindowHandle, Bounds, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use crate::model::{PaneKind, SplitAxis, Tab};
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

/// The checkout's tabs, each by its panes' ids, and the one shown.
fn tabs(shell: &Entity<Shell>, cx: &mut TestAppContext) -> (Vec<Vec<u64>>, usize) {
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().expect("a checkout is selected");
        let tabs = unit.tabs.iter();
        let ids = tabs.map(|tab| tab.panes().iter().map(|pane| pane.id).collect());
        (ids.collect(), unit.active_tab)
    })
}

/// Where the tab at `at` is listed.
fn listed(window: AnyWindowHandle, at: usize, cx: &mut TestAppContext) -> Bounds<Pixels> {
    let selector: &'static str = Box::leak(format!("tab-0-0-{at}").into_boxed_str());
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.debug_bounds(selector).expect("the tab is listed")
}

/// Takes the tab at `from` up by its middle and lets it go at `to`.
fn drag(window: AnyWindowHandle, from: usize, to: Point<Pixels>, cx: &mut TestAppContext) {
    let none = Modifiers::default();
    let held = listed(window, from, cx).center();
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_down(held, MouseButton::Left, none);
    let nudged = held + point(px(3.), px(3.));
    vcx.simulate_mouse_move(nudged, Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_move(to, Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(window, cx);
}

/// In `layout`, a tab held over the middle of another joins it on the side the pointer's
/// on, and one held past the edge of another moves past it.
fn tabs_join_and_reorder(layout: Layout, cx: &mut TestAppContext) {
    let root = repo(&format!("tab-list-{layout:?}"), &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    cx.update(|cx| {
        settings::update(cx, |settings| settings.layout = layout);
        // Tabs go straight to their places, rather than sliding there in time.
        cx.set_reduce_motion(true);
    });
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| {
        for kind in [PaneKind::Chat, PaneKind::Terminal, PaneKind::Chat] {
            shell.workspace.new_tab(kind);
        }
        cx.notify();
    });
    draw(window, cx);
    let (before, _) = tabs(&shell, cx);
    let [a, b, c] = [before[0][0], before[1][0], before[2][0]];

    // Over the second tab's middle, right of its centre: it's joined on its right.
    let onto = listed(window, 1, cx);
    let right = onto.center() + point(onto.size.width * 0.1, px(0.));
    drag(window, 0, right, cx);
    assert_eq!(
        tabs(&shell, cx),
        (vec![vec![b, a], vec![c]], 0),
        "{layout:?}"
    );
    shell.read_with(cx, |shell, _| {
        let tab = &shell.workspace.branch().unwrap().tabs[0];
        assert!(matches!(tab, Tab::Split(split) if split.axis == SplitAxis::Row));
    });

    // Past the start of the first tab: it's moved before it.
    let first = listed(window, 0, cx);
    drag(window, 1, first.origin + point(px(2.), px(2.)), cx);
    assert_eq!(
        tabs(&shell, cx),
        (vec![vec![c], vec![b, a]], 0),
        "{layout:?}"
    );
}

#[gpui_kit::test]
fn orca_strip_tabs_join_and_reorder_by_dragging(cx: &mut TestAppContext) {
    tabs_join_and_reorder(Layout::Orca, cx);
}

#[gpui_kit::test]
fn arc_sidebar_tabs_join_and_reorder_by_dragging(cx: &mut TestAppContext) {
    tabs_join_and_reorder(Layout::Arc, cx);
}

/// However many tabs a checkout has, the strip narrows them to fit beside its + button: each
/// stays on screen to pick, and none runs over the button or what's beside the strip.
#[gpui_kit::test]
fn orca_strip_narrows_its_tabs_to_fit_beside_the_new_tab_button(cx: &mut TestAppContext) {
    let root = repo("tab-list-narrowing", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Orca));
    select(&shell, &root, cx);
    for count in [8, 40] {
        shell.update(cx, |shell, cx| {
            let shown = |shell: &Shell| shell.workspace.branch().map_or(0, |unit| unit.tabs.len());
            while shown(shell) < count {
                shell.workspace.new_tab(PaneKind::Chat);
            }
            cx.notify();
        });
        draw(window, cx);
        let mut vcx = VisualTestContext::from_window(window, cx);
        let button = vcx.debug_bounds("new-tab-button").expect("the + button is shown");
        assert!(button.size.width > px(0.), "with {count} tabs the + button has no width");
        let last = listed(window, count - 1, cx);
        assert!(
            last.right() <= button.left(),
            "with {count} tabs the last ends at {:?}, past the + button at {:?}",
            last.right(),
            button.left()
        );
        for at in 0..count {
            let tab = listed(window, at, cx);
            assert!(tab.size.width > px(0.), "tab {at} of {count} has no width");
        }
    }
}
