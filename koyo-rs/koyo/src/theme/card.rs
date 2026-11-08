//! The islands the window's content sits on in the Arc layout: each pane of the tab shown, the
//! panel beside them, what's listed under them, each a rounded card of its own, the window's
//! glass between them as a thin margin (`docs/arc-sidebar.md` 10). The Orca layout has one card,
//! an island holding them all, hairlines between them. And where each island is in its window,
//! for what's drawn in it in a native view, which an island's rounded corners don't clip as they
//! do GPUI's own drawing: a page's view rounds the corners it shares with its island.

use std::collections::HashMap;

use gpui_kit::base::ElementExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, BoxShadow, Corners, Div, ElementId, EmptyView,
    Global, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Window, WindowId,
    deferred, div, px,
};

use super::{CARD_RADIUS, Palette};

/// How far an edge can lie from an island's and still be its, for layout's rounding.
const NEAR: f32 = 1.;

/// Each window's islands, where they were last laid out.
#[derive(Default)]
struct Islands(HashMap<WindowId, Vec<Bounds<Pixels>>>);

impl Global for Islands {}

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
/// what it holds, which notes where it is as it's laid out, before what's in it is drawn. It's
/// `relative`, so what's in it can be placed over it.
pub fn island(p: Palette) -> Div {
    div()
        .relative()
        .min_w_0()
        .min_h_0()
        .rounded(px(CARD_RADIUS))
        .bg(p.card)
        .shadow(vec![SHADOW])
        .overflow_hidden()
        .on_prepaint(note)
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
/// side of it to drag `payload`. The part picked up is among the window's overlays, so a page
/// either side, in a native layer over the window, doesn't hide the part of it over the page.
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

/// Forgets where `window`'s islands were, as its content is about to be laid out again and each
/// of them notes itself anew. Call it as the window's root renders.
pub fn forget(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if let Some(islands) = cx.default_global::<Islands>().0.get_mut(&id) {
        islands.clear();
    }
}

/// Notes an island laid out at `bounds`, so what's in it knows where it is.
fn note(bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let islands = cx.default_global::<Islands>().0.entry(id).or_default();
    if !islands.contains(&bounds) {
        islands.push(bounds);
    }
}

/// Which corners of `bounds` are those of the island it's on in `window`: the smallest island
/// holding it, as the one it's drawn in is.
pub fn shared_corners(bounds: Bounds<Pixels>, window: &Window, cx: &App) -> Corners<bool> {
    let id = window.window_handle().window_id();
    let islands = cx
        .try_global::<Islands>()
        .and_then(|islands| islands.0.get(&id));
    let Some(island) = islands.and_then(|islands| holding(islands, bounds)) else {
        return Corners::default();
    };
    let near = |a: Pixels, b: Pixels| (a - b).abs() <= px(NEAR);
    let (left, right) = (
        near(bounds.left(), island.left()),
        near(bounds.right(), island.right()),
    );
    let (top, bottom) = (
        near(bounds.top(), island.top()),
        near(bounds.bottom(), island.bottom()),
    );
    Corners {
        top_left: top && left,
        top_right: top && right,
        bottom_right: bottom && right,
        bottom_left: bottom && left,
    }
}

/// The smallest of `islands` that holds `bounds`, give or take layout's rounding.
fn holding(islands: &[Bounds<Pixels>], bounds: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
    (islands.iter())
        .filter(|island| bounds.is_contained_within(&island.dilate(px(NEAR))))
        .min_by(|a, b| {
            let area =
                |bounds: &&Bounds<Pixels>| bounds.size.width.as_f32() * bounds.size.height.as_f32();
            area(a).total_cmp(&area(b))
        })
        .copied()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, point, px, size};

    use super::holding;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn a_page_is_on_the_smallest_island_holding_it() {
        let whole = bounds(0., 0., 1000., 800.);
        let left = bounds(0., 0., 496., 800.);
        let right = bounds(504., 0., 496., 800.);
        let islands = [whole, left, right];
        // A page filling the right pane, under its header.
        let page = bounds(504., 28., 496., 772.);
        assert_eq!(holding(&islands, page), Some(right));
        assert_eq!(holding(&islands, bounds(2000., 0., 10., 10.)), None);
    }
}
