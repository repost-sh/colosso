//! Responsive mode's shared chrome: where content sits in its area, the grips round it and
//! the sizes their drags make in a page's pixels and a terminal's cells, the zoom's words, and
//! a view taking, dragging, double-clicking and letting go of a grip.

use std::ops::RangeInclusive;

use gpui_kit::{
    App, AppContext as _, Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    TestAppContext, bounds, point, px, size,
};

use super::*;

#[test]
fn content_shows_centred_at_the_top_past_the_gutter() {
    let area = bounds(point(px(100.), px(50.)), size(px(1000.), px(900.)));
    let shown = place(area, 200., 400.);
    assert_eq!(shown.size, size(px(200.), px(400.)));
    assert_eq!(shown.origin, point(px(100. + 400.), px(50. + GUTTER)));
    let wide = place(area, 3000., 800.);
    assert_eq!(
        wide.origin.x,
        px(100. + GUTTER),
        "too wide, it keeps its left edge in sight"
    );
}

#[test]
fn grips_sit_outside_the_content_and_size_a_page_within_its_limits() {
    let shown = bounds(point(px(100.), px(100.)), size(px(200.), px(400.)));
    for grip in [Grip::Right, Grip::Bottom, Grip::Corner] {
        let at = grip.bounds(shown);
        assert!(!shown.intersects(&at), "{grip:?} is outside the content");
    }
    assert_eq!(
        Grip::Right.bounds(shown),
        bounds(point(px(304.), px(284.)), size(px(8.), px(32.)))
    );
    assert_eq!(
        Grip::Bottom.bounds(shown),
        bounds(point(px(184.), px(504.)), size(px(32.), px(8.)))
    );
    assert_eq!(
        Grip::Corner.bounds(shown),
        bounds(point(px(304.), px(504.)), size(px(8.), px(8.)))
    );
    // A page shown at half its size: a unit is half a window pixel.
    let limits = 50..=8192;
    let drag = Drag {
        grip: Grip::Right,
        from: point(px(300.), px(300.)),
        at: point(px(300.), px(300.)),
        width: 400,
        height: 800,
        unit: (0.5, 0.5),
    };
    // 10 px in the window is 20 at half, and the right edge moves by half the change.
    assert_eq!(drag.size_at(point(px(310.), px(350.)), &limits), (440, 800));
    let corner = Drag {
        grip: Grip::Corner,
        ..drag
    };
    assert_eq!(corner.size_at(point(px(310.), px(310.)), &limits), (440, 820));
    let bottom = Drag {
        grip: Grip::Bottom,
        ..drag
    };
    assert_eq!(bottom.size_at(point(px(0.), px(0.)), &limits), (400, 200));
    assert_eq!(
        drag.size_at(point(px(-1000.), px(0.)), &limits),
        (50, 800),
        "stops at the limit"
    );
    assert_eq!(drag.size_at(point(px(9000.), px(0.)), &limits), (8192, 800));
}

#[test]
fn a_terminals_grips_size_it_in_whole_cells_within_its_limits() {
    // Cells of 8 × 16 window pixels, from 80 × 24.
    let limits = 2..=1000;
    let corner = Drag {
        grip: Grip::Corner,
        from: point(px(100.), px(100.)),
        at: point(px(100.), px(100.)),
        width: 80,
        height: 24,
        unit: (8., 16.),
    };
    // Twice the pointer's way across: 20 px is 40 px, five columns; 32 px down, two rows.
    assert_eq!(corner.size_at(point(px(120.), px(132.)), &limits), (85, 26));
    assert_eq!(corner.size_at(point(px(97.), px(107.)), &limits), (79, 24));
    let bottom = Drag {
        grip: Grip::Bottom,
        ..corner
    };
    assert_eq!(bottom.size_at(point(px(200.), px(67.)), &limits), (80, 22));
    assert_eq!(
        corner.size_at(point(px(-1000.), px(-1000.)), &limits),
        (2, 2),
        "the limits stop a drag"
    );
    let right = Drag {
        grip: Grip::Right,
        width: 999,
        ..corner
    };
    assert_eq!(right.size_at(point(px(500.), px(100.)), &limits), (1000, 24));
}

#[test]
fn the_zoom_says_fit_with_the_percent_shown_or_its_own_percent() {
    assert_eq!(Zoom::Fit.label(83), "Fit (83%)");
    assert_eq!(Zoom::Percent(50).label(83), "50%");
    let rows: Vec<String> = Zoom::ALL.into_iter().map(Zoom::row).collect();
    assert_eq!(rows, ["Fit", "50%", "75%", "100%"]);
}

/// Content a test sizes: 100 × 50 units of 2 window pixels, filling 300 × 200, a preset when
/// `fixed`.
struct Sized {
    size: (u32, u32),
    fixed: bool,
    drag: Option<Drag>,
    held: Vec<bool>,
}

impl Resizable for Sized {
    fn drag(&mut self) -> &mut Option<Drag> {
        &mut self.drag
    }

    fn grip_size(&self, _: &App) -> Option<((u32, u32), (f32, f32))> {
        (!self.fixed).then_some((self.size, (2., 2.)))
    }

    fn side_limits(&self, _: &App) -> RangeInclusive<u32> {
        10..=500
    }

    fn filling_size(&self, _: &App) -> Option<(u32, u32)> {
        Some((300, 200))
    }

    fn grip_sized(&mut self, width: u32, height: u32, _: &mut Context<Self>) {
        self.size = (width, height);
    }

    fn grip_held(&mut self, _: &mut Context<Self>) {
        let held = self.drag.is_some();
        self.held.push(held);
    }
}

fn press(at: f32, clicks: usize) -> MouseDownEvent {
    MouseDownEvent {
        position: point(px(at), px(at)),
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: clicks,
        first_mouse: false,
    }
}

fn moved(at: f32, held: bool) -> MouseMoveEvent {
    MouseMoveEvent {
        position: point(px(at), px(at)),
        pressed_button: held.then_some(MouseButton::Left),
        modifiers: Modifiers::default(),
    }
}

#[gpui_kit::test]
fn a_grip_is_taken_dragged_and_let_go_and_a_double_click_fits_its_side(cx: &mut TestAppContext) {
    let content = cx.new(|_| Sized {
        size: (100, 50),
        fixed: false,
        drag: None,
        held: Vec::new(),
    });
    let read = |cx: &mut TestAppContext| content.read_with(cx, |it, _| (it.size, it.drag.clone()));
    content.update(cx, |it, cx| it.take_grip(Grip::Corner, &press(10., 1), cx));
    let (_, drag) = read(cx);
    assert_eq!(drag.as_ref().map(|drag| drag.unit), Some((2., 2.)));
    // 20 window pixels is 10 units: 20 across (the right edge moves half), 10 down.
    content.update(cx, |it, cx| it.drag_grip(&moved(30., true), cx));
    assert_eq!(read(cx).0, (120, 60));
    // The button came up outside the window: the next move lets go, and sizes nothing.
    content.update(cx, |it, cx| it.drag_grip(&moved(90., false), cx));
    let (size, drag) = read(cx);
    assert_eq!((size, drag), ((120, 60), None));
    content.update(cx, Sized::let_go_of_grip);
    assert_eq!(
        content.read_with(cx, |it, _| it.held.clone()),
        [true, false],
        "told once each way"
    );
    // A double click fits that side alone to the area.
    content.update(cx, |it, cx| it.take_grip(Grip::Right, &press(10., 2), cx));
    assert_eq!(read(cx), ((300, 60), None));
    content.update(cx, |it, cx| it.take_grip(Grip::Bottom, &press(10., 2), cx));
    assert_eq!(read(cx).0, (300, 200));
    // A preset has no grips to take.
    content.update(cx, |it, cx| {
        it.fixed = true;
        it.take_grip(Grip::Corner, &press(10., 1), cx);
    });
    assert_eq!(read(cx), ((300, 200), None));
}
