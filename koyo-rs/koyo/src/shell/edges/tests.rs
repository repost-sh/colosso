use gpui_kit::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

use crate::budget::{Built, built_after, built_during, ratchet, settle};
use crate::settings::{self, Layout};
use crate::test_support::{GRAB, center_of, move_held, scratch, shell_on};

/// How many moves the drag is measured over, each 10 points further right.
const MOVES: usize = 5;

fn set_width(cx: &TestAppContext) -> f32 {
    cx.read(|cx| settings::get(cx).sidebar_width())
}

/// A drag of the sidebar's edge over several moves: the edge follows the pointer, and the
/// settings are written once, as it's let go, and saved a moment later, which builds nothing.
/// Each move draws the window in full, as GPUI draws it at each move of any drag so what's
/// dragged can follow the pointer: what the settings written at each move cost on top of that
/// is no frame but every observer of them told.
#[gpui_kit::test]
fn a_dragged_sidebar_edge_follows_the_pointer_and_writes_the_settings_once(
    cx: &mut TestAppContext,
) {
    let (window, _, _) = shell_on("sidebar-drag", Layout::Orca, cx);
    let file = cx.update(|cx| settings::tests::saved_in(&scratch("sidebar-drag-settings"), cx));
    settle(cx);
    let (heard, _observing) = settings::tests::hearing(cx);
    let set = set_width(cx);
    let from = center_of(window, "sidebar-resize", cx);
    let at = |step: usize| point(from.x + px(10. * step as f32), from.y);
    VisualTestContext::from_window(window, cx).simulate_mouse_down(
        from,
        MouseButton::Left,
        Modifiers::none(),
    );
    move_held(window, at(1), cx);

    let moves = |cx: &mut TestAppContext| {
        for step in 2..=MOVES + 1 {
            move_held(window, at(step), cx);
        }
    };
    let per_move = Built {
        shell: MOVES,
        views: 11 * MOVES as u64,
        frames: MOVES as u64,
    };
    ratchet(built_after(window, cx, moves), per_move, "the edge moved");
    let edge = center_of(window, "sidebar-resize", cx).x - at(MOVES + 1).x;
    assert!(
        edge.as_f32().abs() < GRAB,
        "the edge is {edge} from the pointer"
    );
    assert_eq!(
        set_width(cx).to_bits(),
        set.to_bits(),
        "nothing's set mid-drag"
    );
    assert_eq!(heard.get(), 0, "nothing's heard mid-drag");

    let let_go = |cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx).simulate_mouse_up(
            at(MOVES + 1),
            MouseButton::Left,
            Modifiers::none(),
        );
    };
    let once = Built {
        shell: 1,
        views: 10,
        frames: 1,
    };
    ratchet(built_after(window, cx, let_go), once, "the edge let go");
    assert_eq!(heard.get(), 1, "the settings are written once");
    let dragged = set_width(cx) - set;
    assert!(
        (dragged - 10. * (MOVES + 1) as f32).abs() < GRAB,
        "it widened {dragged}"
    );

    ratchet(
        built_during(window, cx, settings::SAVE_DELAY),
        Built::default(),
        "the settings saved",
    );
    assert_eq!(heard.get(), 1, "the save tells no one");
    let saved: settings::Settings = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved.sidebar_width.to_bits(),
        set_width(cx).to_bits(),
        "the width is saved"
    );
}
