//! The islands the window's content sits on in the Arc layout: each pane of the tab shown, the
//! panel beside them, what's listed under them, each a rounded card of its own, the window's
//! glass between them as a thin margin (`docs/arc-sidebar.md` 10). The Orca layout has one card,
//! an island holding them all, hairlines between them.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, BoxShadow, Div, ElementId, EmptyView, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, deferred, div, px,
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
        gap(id, row, payload).into_any_element()
    } else {
        divider(id.clone(), id, row, payload, p).into_any_element()
    }
}

/// The line between two parts, `row` when they sit side by side, picked up a few pixels either
/// side of it to drag `payload`. The part picked up is among the window's overlays, so the
/// parts either side, drawn after it, don't hide the part of it that reaches over them.
pub fn divider<T: Clone + 'static>(
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
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView));
    let line = div().flex_shrink_0().relative().bg(p.hairline);
    if row {
        line.w(px(1.)).h_full().child(deferred(handle))
    } else {
        line.h(px(1.)).w_full().child(deferred(handle))
    }
}

/// The glass between two parts, each on an island of its own, `row` when they sit side by
/// side: picked up, it drags `payload`, resizing them.
fn gap<T: Clone + 'static>(id: SharedString, row: bool, payload: T) -> Stateful<Div> {
    let selector = id.clone();
    div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .flex_shrink_0()
        .map(|gap| {
            if row {
                gap.w(px(super::GUTTER)).h_full().cursor_col_resize()
            } else {
                gap.h(px(super::GUTTER)).w_full().cursor_row_resize()
            }
        })
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView))
}
