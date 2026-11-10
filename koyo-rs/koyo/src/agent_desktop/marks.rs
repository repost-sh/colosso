//! The agent's pointer over the desktop (`docs/agent-desktop.md` 9), the arrow a browser
//! page's agent has: it glides to where the host says the agent acted, sends out a ring as a
//! click lands, and fades when the agent rests or the person takes over.

use std::time::Instant;

use gpui_kit::{
    AnyElement, Bounds, ContentMask, Context, IntoElement as _, Pixels, Point, Styled as _, Window,
    canvas, deferred, point, px,
};

use super::canvas::fitted;
use super::view::DeskView;
use crate::browser::pointer::{Next, paint_pointer, tint};

/// The pointer over the desktop, unless there's none to show; asks for the frames and the
/// wake-up it needs to move and fade.
pub fn overlay(
    view: &mut DeskView,
    window: &Window,
    cx: &mut Context<DeskView>,
) -> Option<AnyElement> {
    let now = Instant::now();
    let (shot, moving, desk) = {
        let desk = view.desk.read(cx);
        (desk.pointer.shot(now), desk.pointer.next(now), desk.size()?)
    };
    match moving {
        Next::Frame => window.request_animation_frame(),
        Next::At(at) if view.wake.as_ref().map(|(when, _)| *when) != Some(at) => {
            let wait = at.saturating_duration_since(now);
            let task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            });
            view.wake = Some((at, task));
        }
        Next::At(_) | Next::Never => {}
    }
    let shot = shot?;
    let accent = tint(cx);
    let marks = canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _| {
            let (shown, fit) = fitted(bounds, desk);
            window.with_content_mask(Some(ContentMask { bounds: shown }), |window| {
                let place = |at: Point<f32>| shown.origin + point(px(at.x * fit), px(at.y * fit));
                paint_pointer(&shot, place, accent, window);
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    Some(deferred(marks).into_any_element())
}
