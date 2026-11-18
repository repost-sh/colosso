//! What a device's canvas and the agent desktop's share: the host's screen fitted to the pane
//! and centred, and the pointer over it going to the view. Each lays out and paints its own.

use gpui_kit::{
    Bounds, Context, DispatchPhase, Entity, Hitbox, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, ScrollEvent, Window, point, px, size,
};
use koyo_desk_wire::Size;

/// The view a canvas hands the pointer to.
pub trait Pointed: Sized + 'static {
    /// A press on the screen.
    fn pressed_at(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>);
    /// A release anywhere: the end of a press on the screen, if one is held.
    fn released_at(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>);
    /// A move over the screen (`over`), or anywhere while a press on it is held.
    fn moved_to(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>);
    /// What's scrolled over the screen.
    fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>);
    /// Whether a press on the screen is held.
    fn pressing(&self) -> bool;
}

/// `screen` (in points) at the largest size that fits `area`, centred, never larger than
/// `most` times its size in points: where it shows and how much of its size that is.
pub fn fitted(area: Bounds<Pixels>, screen: Size, most: f32) -> (Bounds<Pixels>, f32) {
    let (wide, high) = (screen.width as f32, screen.height as f32);
    if wide <= 0. || high <= 0. {
        return (area, 1.);
    }
    let fit = (area.size.width.as_f32() / wide)
        .min(area.size.height.as_f32() / high)
        .clamp(0.01, most.max(0.01));
    let shown = size(px(wide * fit), px(high * fit));
    let origin = area.origin
        + point(
            (area.size.width - shown.width) / 2.,
            (area.size.height - shown.height) / 2.,
        );
    (Bounds::new(origin, shown), fit)
}

/// The pointer over `hitbox`, the screen as shown, for `view`: a press on it focuses the tab, a
/// drag that began on it keeps reaching it, and what's scrolled over it goes to the view, not
/// the pane.
pub fn listen<V: Pointed>(view: &Entity<V>, hitbox: &Hitbox, window: &mut Window) {
    let (down, over_down) = (view.downgrade(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !over_down.is_hovered(window) {
            return;
        }
        let _ = down.update(cx, |view, cx| view.pressed_at(event, window, cx));
        cx.stop_propagation();
    });
    let up = view.downgrade();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Capture {
            let _ = up.update(cx, |view, cx| view.released_at(event, cx));
        }
    });
    let (moved, over_move) = (view.downgrade(), hitbox.clone());
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        let over = over_move.is_hovered(window);
        let Some(moved) = moved.upgrade() else {
            return;
        };
        if over || moved.read(cx).pressing() {
            moved.update(cx, |view, cx| view.moved_to(event, over, cx));
        }
    });
    let (scrolled, over_scroll) = (view.downgrade(), hitbox.clone());
    window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !over_scroll.should_handle_scroll(window) {
            return;
        }
        let _ = scrolled.update(cx, |view, cx| view.scrolled(event, cx));
        cx.stop_propagation();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(10.), px(20.)), size(px(width), px(height)))
    }

    #[test]
    fn a_screen_keeps_its_shape_and_is_centred() {
        let desk = Size {
            width: 2000.,
            height: 1000.,
        };
        let (shown, fit) = fitted(area(1200., 400.), desk, 1.);
        assert!((fit - 0.4).abs() < 1e-6);
        assert_eq!(shown.size, size(px(800.), px(400.)));
        assert_eq!(shown.origin, point(px(210.), px(20.)));
        let phone = Size {
            width: 402.,
            height: 874.,
        };
        let (shown, fit) = fitted(area(1000., 437.), phone, 1.5);
        assert!((fit - 0.5).abs() < 1e-6);
        assert_eq!(shown.size.height, px(437.));
        assert_eq!(shown.origin.x, px(10. + (1000. - 201.) / 2.));
    }

    #[test]
    fn a_large_pane_shows_it_no_larger_than_it_may_be() {
        let desk = Size {
            width: 2000.,
            height: 1000.,
        };
        let (shown, fit) = fitted(area(4000., 3000.), desk, 1.);
        assert!((fit - 1.).abs() < 1e-6);
        assert_eq!(shown.size, size(px(2000.), px(1000.)));
        let phone = Size {
            width: 402.,
            height: 874.,
        };
        let (_, fit) = fitted(area(4000., 4000.), phone, 1.5);
        assert!((fit - 1.5).abs() < 1e-6);
    }

    #[test]
    fn a_screen_not_made_yet_takes_the_pane() {
        let (shown, fit) = fitted(area(300., 200.), Size::default(), 1.);
        assert_eq!(shown, area(300., 200.));
        assert!((fit - 1.).abs() < 1e-6);
    }
}
