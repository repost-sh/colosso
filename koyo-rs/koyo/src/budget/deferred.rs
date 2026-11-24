//! What GPUI copies from last frame of a view drawn around nested views that are built again:
//! the draws it deferred outside them, a bar or a popover over it, unless one holds a view that
//! changed or read something that did (gpui-fast's `splice_layout`, `vendor/README.md`). A
//! chat's floating bars are such draws, and each beat of a spinner beside them builds the
//! spinner alone. Each case checks the frame drawn against the same window built whole.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, TestAppContext, Window, deferred, div, px, size,
};

use super::{built_after, on};
use crate::test_support::in_window;

struct Number(usize);

/// A view of a fixed size whose colour comes from its tint and, when it has one, a model.
struct Swatch {
    tint: usize,
    model: Option<Entity<Number>>,
    builds: Rc<Cell<usize>>,
}

impl Render for Swatch {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.builds.set(self.builds.get() + 1);
        let model = self.model.as_ref().map_or(0, |model| model.read(cx).0);
        let colour = if (self.tint + model).is_multiple_of(2) { gpui_kit::black() } else { gpui_kit::white() };
        div().w(px(50.)).h(px(20.)).bg(colour)
    }
}

/// A view with a swatch in its flow and another drawn over it, deferred, or deferred from what
/// it defers when `twice`.
struct Host {
    flow: Entity<Swatch>,
    over: Entity<Swatch>,
    twice: bool,
    builds: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.builds.set(self.builds.get() + 1);
        let over = div().w(px(200.)).h(px(40.)).child(self.over.clone());
        let over = if self.twice { deferred(div().child(deferred(over))) } else { deferred(over) };
        div().size_full().child(self.flow.clone()).child(over)
    }
}

struct Fixture {
    window: AnyWindowHandle,
    flow: Entity<Swatch>,
    over: Entity<Swatch>,
    model: Entity<Number>,
    host: Rc<Cell<usize>>,
    overlay: Rc<Cell<usize>>,
}

impl Fixture {
    fn new(twice: bool, cx: &mut TestAppContext) -> Fixture {
        let model = cx.new(|_| Number(0));
        let swatch = |model| Swatch { tint: 0, model, builds: Rc::default() };
        let flow = cx.new(|_| swatch(None));
        let over = cx.new(|_| swatch(Some(model.clone())));
        let overlay = over.read_with(cx, |over, _| over.builds.clone());
        let host = Rc::new(Cell::new(0));
        let (flow_in, over_in, host_in) = (flow.clone(), over.clone(), host.clone());
        let (window, _) = in_window(size(px(300.), px(100.)), cx, move |_, cx| {
            cx.new(|_| Host { flow: flow_in, over: over_in, twice, builds: host_in })
        });
        Fixture { window, flow, over, model, host, overlay }
    }

    /// How many times the host and the swatch drawn over it were built.
    fn builds(&self) -> (usize, usize) {
        (self.host.get(), self.overlay.get())
    }

    /// Draws what `change` changes, from rest to rest.
    fn after(&self, cx: &mut TestAppContext, change: impl FnOnce(&mut TestAppContext)) {
        built_after(self.window, cx, change);
    }

    /// The quads of the frame last drawn, in a stable order.
    fn quads(&self, cx: &mut TestAppContext) -> Vec<String> {
        let mut quads: Vec<String> = on(self.window, cx, |window, _| {
            (window.painted_quads().iter()).map(|quad| format!("{:?} {:?}", quad.bounds, quad.background)).collect()
        });
        quads.sort();
        quads
    }

    /// The quads of the window built whole, with nothing kept from last frame, as upstream GPUI
    /// draws it.
    fn whole(&self, cx: &mut TestAppContext) -> Vec<String> {
        let over = self.over.entity_id();
        on(self.window, cx, |window, _| window.set_view_retention(false));
        self.after(cx, |cx| cx.update(|cx| cx.notify(over)));
        let whole = self.quads(cx);
        on(self.window, cx, |window, _| window.set_view_retention(true));
        whole
    }
}

/// Has `swatch` take `tint`, in the update `cx` is: changes made in one update are drawn in one
/// frame.
fn tint(swatch: &Entity<Swatch>, tint: usize, cx: &mut gpui_kit::App) {
    swatch.update(cx, |swatch, cx| {
        swatch.tint = tint;
        cx.notify();
    });
}

#[gpui_kit::test]
fn a_view_drawn_around_a_nested_view_copies_what_it_deferred_outside_it(cx: &mut TestAppContext) {
    let fixture = Fixture::new(false, cx);
    let built = fixture.builds();
    for turn in [1, 0, 1] {
        fixture.after(cx, |cx| cx.update(|cx| tint(&fixture.flow, turn, cx)));
        assert_eq!(fixture.builds(), built, "turn {turn}: the host and what it deferred are copied");
    }
    let drawn = fixture.quads(cx);
    assert_eq!(drawn, fixture.whole(cx));
}

#[gpui_kit::test]
fn a_view_drawn_around_a_nested_view_builds_again_what_it_deferred_that_read_a_change(
    cx: &mut TestAppContext,
) {
    let fixture = Fixture::new(false, cx);
    let (host, overlay) = fixture.builds();
    fixture.after(cx, |cx| cx.update(|cx| tint(&fixture.flow, 1, cx)));
    assert_eq!(fixture.builds(), (host, overlay), "what it deferred was copied");

    fixture.after(cx, |cx| {
        cx.update(|cx| {
            fixture.model.update(cx, |model, cx| {
                model.0 = 1;
                cx.notify();
            });
            tint(&fixture.flow, 0, cx);
        });
    });
    assert_eq!(fixture.builds().1, overlay + 1, "the swatch over it read the model, which changed");
    let drawn = fixture.quads(cx);
    assert_eq!(drawn, fixture.whole(cx));
}

#[gpui_kit::test]
fn a_view_drawn_around_a_nested_view_builds_again_a_dirty_view_deferred_twice(cx: &mut TestAppContext) {
    let fixture = Fixture::new(true, cx);
    let (_, overlay) = fixture.builds();
    fixture.after(cx, |cx| {
        cx.update(|cx| {
            tint(&fixture.flow, 1, cx);
            tint(&fixture.over, 1, cx);
        });
    });
    assert_eq!(fixture.builds().1, overlay + 1);
    let drawn = fixture.quads(cx);
    assert_eq!(drawn, fixture.whole(cx));
}
