//! The islands the window's content sits on: each pane of the tab shown, the panel beside them,
//! what's listed under them, each a rounded card of its own, the window's glass between them as a
//! thin margin (`docs/sidebar.md` 10).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Bounds, BoxShadow, Corners, Div, ElementId, EmptyView, Hsla,
    InteractiveElement as _, ParentElement as _, Pixels, Point, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, canvas, div, px,
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
/// what GPUI draws in it. It's `relative`, so what's in it can be placed over it. A native
/// view in it isn't GPUI's to clip: the island says where it is as it's prepainted
/// ([`prepainted`]), which is when such a view is placed, and the view rounds the corners
/// it shares with it.
pub fn island(p: Palette) -> Div {
    div()
        .bg(p.card)
        .relative()
        .min_w_0()
        .min_h_0()
        .rounded(px(CARD_RADIUS))
        .shadow(vec![SHADOW])
        .overflow_hidden()
        .child(
            canvas(|bounds, _, _| PREPAINTED.set(Some(bounds)), |_, (), _, _| {})
                .absolute()
                .size_full(),
        )
        .on_children_prepainted(|_, _, _| PREPAINTED.set(None))
}

thread_local! {
    /// Where the island being prepainted is, from when its prepainting begins until all it
    /// holds is prepainted; none outside one.
    static PREPAINTED: std::cell::Cell<Option<Bounds<Pixels>>> = const { std::cell::Cell::new(None) };
}

/// The island being prepainted: the one what's prepainted now is in, if it's in one.
pub fn prepainted() -> Option<Bounds<Pixels>> {
    PREPAINTED.get()
}

/// Which of the corners of what fills `bounds` are the corners of the island over `island` too,
/// to a point either way: those a native view over `bounds` rounds as the island rounds its own.
pub fn shared_corners(bounds: Bounds<Pixels>, island: Bounds<Pixels>) -> Corners<bool> {
    let near = |inner: Pixels, outer: Pixels| (inner - outer).abs() <= px(1.);
    let (left, right) = (near(bounds.left(), island.left()), near(bounds.right(), island.right()));
    let (top, bottom) = (near(bounds.top(), island.top()), near(bounds.bottom(), island.bottom()));
    Corners {
        top_left: left && top,
        top_right: right && top,
        bottom_right: right && bottom,
        bottom_left: left && bottom,
    }
}

/// What lies between two islands, `row` when they sit side by side, which picking it up drags
/// `payload` to resize them: the glass between them, a gutter wide.
pub fn seam<T: Clone + 'static>(
    id: impl Into<SharedString>,
    row: bool,
    payload: T,
) -> Stateful<Div> {
    let id = id.into();
    band(id.clone(), id, row, payload, super::GUTTER)
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
/// islands. The drag is the app's from the press on, over a native view too: the window
/// hands a button's moves to whoever took its press (`docs/native-pages.md` 6).
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
        .on_drag(payload, |_, _, _, cx| cx.new(|_| EmptyView))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, Corners, point, px, size};

    use super::shared_corners;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    /// Corners from the top left round, as CSS lists them.
    fn corners([top_left, top_right, bottom_right, bottom_left]: [bool; 4]) -> Corners<bool> {
        Corners {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        }
    }

    #[test]
    fn a_page_shares_the_corners_of_its_island_it_reaches_to_a_point_either_way() {
        let island = bounds(10., 40., 800., 600.);
        assert_eq!(shared_corners(island, island), corners([true, true, true, true]), "filling it");
        let under_header = bounds(10., 72., 800., 568.);
        assert_eq!(shared_corners(under_header, island), corners([false, false, true, true]), "under a header");
        let on_half_points = bounds(9.5, 72., 801., 568.5);
        assert_eq!(shared_corners(on_half_points, island), corners([false, false, true, true]), "half a point out");
        let left_half = bounds(10., 40., 400., 600.);
        assert_eq!(shared_corners(left_half, island), corners([true, false, false, true]), "its left half");
        let centred = bounds(200., 100., 390., 400.);
        assert_eq!(shared_corners(centred, island), Corners::default(), "a device centred in it");
        let two_points_in = bounds(12., 42., 796., 596.);
        assert_eq!(shared_corners(two_points_in, island), Corners::default(), "two points in");
    }
}
