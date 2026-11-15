//! The islands the window's content sits on in the Arc layout: each pane of the tab shown, the
//! panel beside them, what's listed under them, each a rounded card of its own, the window's
//! glass between them as a thin margin (`docs/arc-sidebar.md` 10). The Orca layout has one card,
//! an island holding them all, hairlines between them.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, BoxShadow, CursorStyle, DispatchPhase, Div,
    ElementId, EmptyView, Global, Hsla, HitboxBehavior, InteractiveElement as _, IntoElement,
    MouseButton, MouseUpEvent, ParentElement as _, Pixels, Point, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, deferred, div, px,
};

use super::{CARD_RADIUS, Palette};

/// The shadow under an island: barely there, a point's drop and a short blur, so it reads as
/// lifted off the glass only to the eye that looks.
const SHADOW: BoxShadow = BoxShadow {
    color: Hsla {
        h: 0.,
        s: 0.,
        l: 0.,
        a: 0.12,
    },
    offset: Point {
        x: Pixels::ZERO,
        y: px(1.),
    },
    blur_radius: px(3.),
    spread_radius: Pixels::ZERO,
    inset: false,
};

/// An island: a rounded card in the theme's colour, a barely-there shadow under it, clipping
/// what it holds, a browser page's frame included. It's `relative`, so what's in it can be
/// placed over it.
pub fn island(p: Palette) -> Div {
    div()
        .relative()
        .min_w_0()
        .min_h_0()
        .rounded(px(CARD_RADIUS))
        .bg(p.card)
        .shadow(vec![SHADOW])
        .overflow_hidden()
}

/// Whether the layout puts each part on an island of its own: Arc's does; Orca's has one card.
pub fn islands(cx: &App) -> bool {
    crate::settings::get(cx).layout == crate::settings::Layout::Arc
}

/// A part of the window: an island of its own when the layout has `islands`, otherwise plain,
/// on the one card the layout has.
pub fn part(p: Palette, islands: bool) -> Div {
    if islands {
        island(p)
    } else {
        div().relative().min_w_0().min_h_0()
    }
}

/// What lies between two parts, `row` when they sit side by side, which picking it up drags
/// `payload` to resize them: with `islands`, the glass between them, a gutter wide; on the one
/// card, a hairline, grabbed a few pixels either side.
pub fn seam<T: Clone + 'static>(
    id: impl Into<SharedString>,
    row: bool,
    payload: T,
    p: Palette,
    islands: bool,
) -> AnyElement {
    let id = id.into();
    if islands {
        band(id.clone(), id, row, payload, super::GUTTER).into_any_element()
    } else {
        hairline(id.clone(), id, row, payload, p).into_any_element()
    }
}

/// How thick the divider between two parts is, and its grip: how long, how thick.
pub const DIVIDER: f32 = 9.;
const GRIP: (f32, f32) = (28., 4.);

/// The divider between two parts, `row` when they sit side by side, with a grip in its middle
/// as Arc has between a page and its developer tools: picked up anywhere along it, it drags
/// `payload`. It has room of its own between the parts rather than reaching over them: either
/// may be a native view, which takes a press over it before the app's own content does.
pub fn divider<T: Clone + 'static>(
    id: impl Into<ElementId>,
    selector: SharedString,
    row: bool,
    payload: T,
    p: Palette,
) -> Stateful<Div> {
    let grip = div().rounded_full().bg(p.muted.opacity(0.7)).map(|grip| {
        if row {
            grip.w(px(GRIP.1)).h(px(GRIP.0))
        } else {
            grip.w(px(GRIP.0)).h(px(GRIP.1))
        }
    });
    band(id, selector, row, payload, DIVIDER)
        .flex()
        .items_center()
        .justify_center()
        .hover(move |band| band.bg(p.hover))
        .child(grip)
}

/// A band `thickness` thick between two parts, `row` when they sit side by side, with room of
/// its own: picked up, it drags `payload`, resizing them. Bare, it's the glass between two
/// islands.
fn band<T: Clone + 'static>(
    id: impl Into<ElementId>,
    selector: SharedString,
    row: bool,
    payload: T,
    thickness: f32,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .flex_shrink_0()
        .map(|band| {
            if row {
                band.w(px(thickness)).h_full().cursor_col_resize()
            } else {
                band.h(px(thickness)).w_full().cursor_row_resize()
            }
        })
        .map(|band| held(band, deferred(shield(row))))
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView))
}

/// `grabbed`, what a press picks a seam up by, with the pointer the app's all over the window
/// from that press until the button is let go: `shield` is [`shield`], among the window's
/// overlays, where a seam's own handle may be already.
fn held(grabbed: Stateful<Div>, shield: impl IntoElement) -> Stateful<Div> {
    grabbed
        .on_mouse_down(MouseButton::Left, |_, window, cx| hold(true, window, cx))
        // Let go before the shield was drawn: over what was pressed, or off it.
        .on_mouse_up(MouseButton::Left, |_, window, cx| hold(false, window, cx))
        .on_mouse_up_out(MouseButton::Left, |_, window, cx| hold(false, window, cx))
        .child(shield)
}

/// Whether a seam is pressed: a drag of it may be under way.
#[derive(Default)]
struct Held(bool);

impl Global for Held {}

/// Says whether a seam is pressed, and has the window drawn again for it: at once on a press,
/// so the shield is there before the pointer's next move rather than at the screen's next
/// refresh.
fn hold(held: bool, window: &mut Window, cx: &mut App) {
    if cx.default_global::<Held>().0 == held {
        return;
    }
    cx.set_global(Held(held));
    window.refresh();
    #[cfg(all(target_os = "macos", not(test)))]
    if held {
        crate::native::window::draw_in_transaction(window, cx);
    }
}

/// While a seam is pressed, the whole window as the app's own, among its overlays, with the
/// seam's cursor. A native view takes the pointer over it before the app's content does, and
/// GPUI starts a drag only once it has heard the pointer move more than two points from the
/// press: a hand at its usual speed is off a band nine points thick, over the page beside it,
/// by its first move, which was the page's, and nothing was picked up
/// (`docs/native-pages.md` 6). Under the shield every move from the press on is the app's,
/// the first frame's aside, which the drag does without: it goes by where the pointer is.
fn shield(row: bool) -> impl IntoElement {
    canvas(
        |_, window, cx| {
            let held = cx.try_global::<Held>().is_some_and(|held| held.0);
            let all = Bounds::new(Point::default(), window.viewport_size());
            held.then(|| window.insert_hitbox(all, HitboxBehavior::Normal))
        },
        move |_, hitbox, window, _| {
            let Some(hitbox) = hitbox else {
                return;
            };
            let cursor = if row {
                CursorStyle::ResizeColumn
            } else {
                CursorStyle::ResizeRow
            };
            window.set_cursor_style(cursor, &hitbox);
            window.on_mouse_event(|_: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    hold(false, window, cx);
                }
            });
        },
    )
}

/// The line between two parts on the one card, `row` when they sit side by side, picked up a
/// few pixels either side of it to drag `payload`. The part picked up is among the window's
/// overlays, so the parts either side, drawn after it, don't hide the part of it that reaches
/// over them.
fn hairline<T: Clone + 'static>(
    id: impl Into<ElementId>,
    selector: SharedString,
    row: bool,
    payload: T,
    p: Palette,
) -> Div {
    let handle = div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .absolute()
        .occlude()
        .map(|handle| {
            if row {
                handle
                    .top_0()
                    .bottom_0()
                    .left(px(-6.))
                    .w(px(13.))
                    .cursor_col_resize()
            } else {
                handle
                    .left_0()
                    .right_0()
                    .top(px(-6.))
                    .h(px(13.))
                    .cursor_row_resize()
            }
        })
        // Among the overlays already, and its shield with it.
        .map(|handle| held(handle, shield(row)))
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView));
    let line = div().flex_shrink_0().relative().bg(p.hairline);
    if row {
        line.w(px(1.)).h_full().child(deferred(handle))
    } else {
        line.h(px(1.)).w_full().child(deferred(handle))
    }
}
