//! The grips on the right edge, the bottom edge and the corner of content shown at a free
//! size, outside it: dragged, they size it (the right one by twice the pointer's move, the
//! content staying centred); double-clicked, they fit that side to the area. The size shows
//! by the pointer while one is in hand. What they size is the pane's: a page's pixels, a
//! terminal's cells (`Resizable`).

use std::ops::RangeInclusive;

use gpui_kit::{
    AnyElement, App, Bounds, Context, CursorStyle, DispatchPhase, Entity, Hitbox, HitboxBehavior,
    IntoElement as _, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels,
    Point, Styled as _, Window, anchored, deferred, div, fill, point, px, size,
};

use crate::theme::Palette;

/// A grip's thickness, and its length along its edge.
const THICK: f32 = 8.;
const LONG: f32 = 32.;
/// How far off the content's edge a grip sits.
const OFF: f32 = 4.;

/// Which grip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    Right,
    Bottom,
    Corner,
}

impl Grip {
    const ALL: [Grip; 3] = [Grip::Right, Grip::Bottom, Grip::Corner];

    /// Where the grip sits round content shown at `shown`.
    pub fn bounds(self, shown: Bounds<Pixels>) -> Bounds<Pixels> {
        let (right, bottom) = (shown.right() + px(OFF), shown.bottom() + px(OFF));
        let middle = shown.center();
        match self {
            Grip::Right => Bounds::new(
                point(right, middle.y - px(LONG / 2.)),
                size(px(THICK), px(LONG)),
            ),
            Grip::Bottom => Bounds::new(
                point(middle.x - px(LONG / 2.), bottom),
                size(px(LONG), px(THICK)),
            ),
            Grip::Corner => Bounds::new(point(right, bottom), size(px(THICK), px(THICK))),
        }
    }

    fn cursor(self) -> CursorStyle {
        match self {
            Grip::Right => CursorStyle::ResizeLeftRight,
            Grip::Bottom => CursorStyle::ResizeUpDown,
            Grip::Corner => CursorStyle::ResizeUpLeftDownRight,
        }
    }
}

/// A grip in hand: which, where it was taken and where the pointer is, the content's size
/// then in its units, and how many window pixels a unit showed as across and down then.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub grip: Grip,
    pub from: Point<Pixels>,
    pub at: Point<Pixels>,
    pub width: u32,
    pub height: u32,
    pub unit: (f32, f32),
}

impl Drag {
    /// The content's size with the pointer at `to`, each side kept within `limits`.
    pub fn size_at(&self, to: Point<Pixels>, limits: &RangeInclusive<u32>) -> (u32, u32) {
        let moved = to - self.from;
        let (dx, dy) = (
            moved.x.as_f32() / self.unit.0.max(f32::EPSILON),
            moved.y.as_f32() / self.unit.1.max(f32::EPSILON),
        );
        let side = |from: u32, by: f32| {
            let wanted = (from as f32 + by).round().max(0.) as u32;
            wanted.clamp(*limits.start(), *limits.end())
        };
        match self.grip {
            Grip::Right => (side(self.width, 2. * dx), self.height),
            Grip::Bottom => (self.width, side(self.height, dy)),
            Grip::Corner => (side(self.width, 2. * dx), side(self.height, dy)),
        }
    }
}

/// A view whose content the grips size: a page, a terminal. It says what the grips need to
/// know; taking, dragging and letting go of them is the same for every one.
pub trait Resizable: Sized + 'static {
    /// The grip in hand, if one is.
    fn drag(&mut self) -> &mut Option<Drag>;
    /// The content's size and the window pixels a unit shows as, across and down, while it's
    /// a free size the grips drag; `None` otherwise.
    fn grip_size(&self, cx: &App) -> Option<((u32, u32), (f32, f32))>;
    /// What a side is kept within.
    fn side_limits(&self, cx: &App) -> RangeInclusive<u32>;
    /// The size that fills the area at the content's own scale, a double click's.
    fn filling_size(&self, cx: &App) -> Option<(u32, u32)>;
    /// The person sized the content to `width` × `height`.
    fn grip_sized(&mut self, width: u32, height: u32, cx: &mut Context<Self>);
    /// A grip was taken or let go.
    fn grip_held(&mut self, _held: bool, _cx: &mut Context<Self>) {}

    /// A press on `grip`: a double click fits that side to the area, a single one takes it.
    fn take_grip(&mut self, grip: Grip, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(((width, height), unit)) = self.grip_size(cx) else {
            return;
        };
        if event.click_count >= 2 {
            if self.drag().take().is_some() {
                self.grip_held(false, cx);
            }
            let Some((wide, tall)) = self.filling_size(cx) else {
                return;
            };
            let (width, height) = match grip {
                Grip::Right => (wide, height),
                Grip::Bottom => (width, tall),
                Grip::Corner => (wide, tall),
            };
            self.grip_sized(width, height, cx);
            return;
        }
        *self.drag() = Some(Drag {
            grip,
            from: event.position,
            at: event.position,
            width,
            height,
            unit,
        });
        self.grip_held(true, cx);
        cx.notify();
    }

    /// The pointer moved with a grip in hand: the content takes the size it gives.
    fn drag_grip(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let limits = self.side_limits(cx);
        let Some(drag) = self.drag().as_mut() else {
            return;
        };
        if event.pressed_button.is_none() {
            // The release happened outside the window.
            self.let_go_of_grip(cx);
            return;
        }
        let to = event.position;
        drag.at = to;
        let (width, height) = drag.size_at(to, &limits);
        self.grip_sized(width, height, cx);
        cx.notify();
    }

    fn let_go_of_grip(&mut self, cx: &mut Context<Self>) {
        if self.drag().take().is_some() {
            self.grip_held(false, cx);
            cx.notify();
        }
    }
}

/// The grips round content shown at `shown`, as hitboxes.
pub fn hitboxes(shown: Bounds<Pixels>, window: &mut Window) -> Vec<(Grip, Hitbox)> {
    (Grip::ALL.into_iter())
        .map(|grip| {
            (
                grip,
                window.insert_hitbox(grip.bounds(shown), HitboxBehavior::Normal),
            )
        })
        .collect()
}

/// Paints the grips and has them heard: presses take one, and the window's moves and
/// releases drag it until it's let go. `held` is the grip in hand.
pub fn paint<V: Resizable>(
    grips: &[(Grip, Hitbox)],
    held: Option<Grip>,
    view: &Entity<V>,
    p: Palette,
    window: &mut Window,
) {
    for (grip, hitbox) in grips {
        let lit = held == Some(*grip) || hitbox.is_hovered(window);
        window.paint_quad(
            fill(hitbox.bounds, if lit { p.muted } else { p.hairline }).corner_radii(px(3.)),
        );
        window.set_cursor_style(grip.cursor(), hitbox);
        let (grip, hitbox, pressed) = (*grip, hitbox.clone(), view.downgrade());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                let _ = pressed.update(cx, |view, cx| view.take_grip(grip, event, cx));
                cx.stop_propagation();
            }
        });
    }
    let moved = view.downgrade();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx: &mut App| {
        if phase == DispatchPhase::Capture {
            let _ = moved.update(cx, |view, cx| view.drag_grip(event, cx));
        }
    });
    let released = view.downgrade();
    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx: &mut App| {
        if phase == DispatchPhase::Capture {
            let _ = released.update(cx, V::let_go_of_grip);
        }
    });
}

/// The content's size, `width` × `height`, by the pointer while `drag` is in hand.
pub fn label(drag: Option<&Drag>, width: u32, height: u32, p: Palette) -> Option<AnyElement> {
    let drag = drag?;
    let tag = div()
        .px(px(6.))
        .py(px(2.))
        .rounded(px(4.))
        .bg(p.floating())
        .border_1()
        .border_color(p.hairline)
        .text_xs()
        .text_color(p.text)
        .child(format!("{width} × {height}"));
    let at = anchored()
        .position(drag.at + point(px(14.), px(18.)))
        .child(tag);
    Some(deferred(at).with_priority(1).into_any_element())
}
