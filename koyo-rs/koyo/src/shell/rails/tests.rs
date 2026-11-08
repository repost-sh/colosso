//! ⌘B folds every rail as thin as the gutter with the sidebar, compact mode, and brings them
//! back, moving rather than jumping; the status bar is tall only with the sidebar out and
//! servers to show; and the traffic lights are gone once the top rail is too thin to hold them.

use std::time::Duration;

use gpui_kit::{
    AnimationPhase, AnyWindowHandle, Bounds, Entity, Pixels, TestAppContext, VisualTestContext,
    point, px,
};

use super::{LIGHTS_GONE, lights, thickness};
use crate::files::tests::press;
use crate::settings::{self, Layout};
use crate::shell::Shell;
use crate::test_support::{draw, shell_with};
use crate::theme;

/// The test's window is 1400 × 900.
const WINDOW: (f32, f32) = (1400., 900.);

/// Where the card is in `window`.
fn card(window: AnyWindowHandle, cx: &mut TestAppContext) -> Bounds<Pixels> {
    VisualTestContext::from_window(window, cx)
        .debug_bounds("shell-card")
        .unwrap()
}

/// Where the card is in compact mode: a gutter in from every edge.
fn compact() -> Bounds<Pixels> {
    let gutter = px(theme::GUTTER);
    let far = point(px(WINDOW.0) - gutter, px(WINDOW.1) - gutter);
    Bounds::from_corners(point(gutter, gutter), far)
}

/// Draws `window` again once `shell` has moved on by `by`, as its next animation frame would.
fn after(by: Duration, window: AnyWindowHandle, shell: &Entity<Shell>, cx: &mut TestAppContext) {
    cx.executor().advance_clock(by);
    shell.update(cx, |_, cx| cx.notify());
    draw(window, cx);
}

#[gpui_kit::test]
fn cmd_b_folds_every_rail_as_thin_as_the_gutter_and_brings_them_back(cx: &mut TestAppContext) {
    let (window, _shell) = shell_with(&[], cx);
    // The springs land at once, so each draw shows where they were sent.
    cx.update(|cx| cx.set_reduce_motion(true));
    for layout in Layout::ALL {
        cx.update(|cx| settings::update(cx, |settings| settings.layout = layout));
        draw(window, cx);
        // Out: the top and icon rails full, Arc's sidebar on the glass between the icon rail and
        // the card; the status bar as thin as the gutter, with no servers to show.
        let out = card(window, cx);
        let sidebar = match layout {
            Layout::Orca => 0.,
            Layout::Arc => cx.read(|cx| settings::get(cx).sidebar_width()),
        };
        let left = px(theme::GUTTER + sidebar);
        assert_eq!(
            out.origin,
            point(left, px(theme::TITLE_BAR_HEIGHT)),
            "{layout:?}"
        );
        assert_eq!(
            (out.right(), out.bottom()),
            (compact().right(), compact().bottom())
        );

        press(window, "cmd-b", cx);
        draw(window, cx);
        assert_eq!(card(window, cx), compact(), "{layout:?} compact");

        press(window, "cmd-b", cx);
        draw(window, cx);
        assert_eq!(card(window, cx), out, "{layout:?} out again");
    }
}

#[gpui_kit::test]
fn the_rails_move_to_compact_mode_rather_than_jump(cx: &mut TestAppContext) {
    let (window, shell) = shell_with(&[], cx);
    // In Arc the sidebar is left of the card, which slides with it; in Orca, with no rail down
    // the left any more, the card's left edge stays at the gutter.
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Arc));
    draw(window, cx);
    let out = card(window, cx);

    press(window, "cmd-b", cx);
    draw(window, cx);
    after(Duration::from_millis(60), window, &shell, cx);
    let between = card(window, cx);
    assert!(compact().origin.x < between.origin.x && between.origin.x < out.origin.x);
    assert!(compact().origin.y < between.origin.y && between.origin.y < out.origin.y);

    after(Duration::from_secs(1), window, &shell, cx);
    assert_eq!(card(window, cx), compact());
}

#[gpui_kit::test]
fn the_status_bar_is_out_only_with_the_sidebar_out_and_servers_to_show(cx: &mut TestAppContext) {
    let (window, shell) = shell_with(&[], cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.rails.servers = true;
        cx.notify();
    });
    draw(window, cx);
    let status = px(WINDOW.1 - theme::STATUS_BAR_HEIGHT);
    assert_eq!(card(window, cx).bottom(), status, "out, with servers");

    press(window, "cmd-b", cx);
    draw(window, cx);
    assert_eq!(card(window, cx), compact(), "compact, with servers");

    press(window, "cmd-b", cx);
    shell.update(cx, |shell, cx| {
        shell.rails.servers = false;
        cx.notify();
    });
    draw(window, cx);
    assert_eq!(
        card(window, cx).bottom(),
        compact().bottom(),
        "out, with none"
    );
}

#[test]
fn the_traffic_lights_are_gone_once_the_top_rail_is_too_thin_to_hold_them() {
    let holds = theme::TRAFFIC_LIGHTS_TOP + theme::TRAFFIC_LIGHT_SIZE;
    let gone = AnimationPhase(LIGHTS_GONE);
    assert!((thickness(theme::TITLE_BAR_HEIGHT, gone) - holds).abs() < 1e-4);
    for (open, shown) in [
        (0., 0.),
        (LIGHTS_GONE, 0.),
        (f32::midpoint(LIGHTS_GONE, 1.), 0.5),
        (1., 1.),
    ] {
        assert!(
            (lights(AnimationPhase(open)) - shown).abs() < 1e-6,
            "{open}"
        );
    }
}
