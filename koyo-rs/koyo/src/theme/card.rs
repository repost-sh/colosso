//! Where the content card is in each window, for what's drawn in it in a native view, which
//! the card's rounded corners don't clip as they do GPUI's own drawing: a page's view rounds
//! the corners it shares with the card itself.

use std::collections::HashMap;

use gpui_kit::{
    App, Bounds, Corners, Global, IntoElement, Pixels, Styled as _, Window, WindowId, canvas, px,
};

/// How far an edge can lie from the card's and still be its, for layout's rounding.
const NEAR: f32 = 1.;

/// Each window's card, where it was last laid out.
#[derive(Default)]
struct Cards(HashMap<WindowId, Bounds<Pixels>>);

impl Global for Cards {}

/// What to put in the card, filling it, so what's in it knows where it is: it notes the
/// card's bounds as it's laid out, before what's in it is drawn.
pub fn noted() -> impl IntoElement {
    canvas(
        |bounds, window, cx| {
            let id = window.window_handle().window_id();
            cx.default_global::<Cards>().0.insert(id, bounds);
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Which corners of `bounds` are the card's own in `window`.
pub fn shared_corners(bounds: Bounds<Pixels>, window: &Window, cx: &App) -> Corners<bool> {
    let id = window.window_handle().window_id();
    let Some(card) = cx.try_global::<Cards>().and_then(|cards| cards.0.get(&id)) else {
        return Corners::default();
    };
    let near = |a: Pixels, b: Pixels| (a - b).abs() <= px(NEAR);
    let (left, right) = (
        near(bounds.left(), card.left()),
        near(bounds.right(), card.right()),
    );
    let (top, bottom) = (
        near(bounds.top(), card.top()),
        near(bounds.bottom(), card.bottom()),
    );
    Corners {
        top_left: top && left,
        top_right: top && right,
        bottom_right: bottom && right,
        bottom_left: bottom && left,
    }
}
