//! The desktop itself, as an element (`docs/agent-desktop.md` 9): the desktop's shape fitted
//! to the pane, each window's latest frame drawn at its place, back to front as the host
//! lists them, and every pointer event over it. The desktop counts as on screen while the
//! element is drawn (`Shown`, in its state) in a window seen: the host sends its frames only
//! then.

use gpui_kit::{
    App, Bounds, ContentMask, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, LayoutId, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, Style, Window, fill, point, px, relative,
    size,
};

use super::Desk;
use super::view::DeskView;
use crate::sight::Shown;
use crate::theme::Palette;

pub struct Canvas {
    pub view: Entity<DeskView>,
    pub desk: Entity<Desk>,
    pub focused: bool,
    pub palette: Palette,
}

/// Where the desktop shows in the element.
pub struct Placed {
    hitbox: Hitbox,
    shown: Bounds<Pixels>,
    /// How much of its true size it shows at.
    fit: f32,
}

/// `desk` (in points) at the largest size that fits `area`, centred in it, never larger than
/// it is: where it shows, and how much of its true size that is.
pub fn fitted(area: Bounds<Pixels>, desk: koyo_desk_wire::Size) -> (Bounds<Pixels>, f32) {
    let (wide, high) = (desk.width as f32, desk.height as f32);
    if wide <= 0. || high <= 0. {
        return (area, 1.);
    }
    let fit = (area.size.width.as_f32() / wide)
        .min(area.size.height.as_f32() / high)
        .clamp(0.01, 1.);
    let shown = size(px(wide * fit), px(high * fit));
    let origin = area.origin
        + point(
            (area.size.width - shown.width) / 2.,
            (area.size.height - shown.height) / 2.,
        );
    (Bounds::new(origin, shown), fit)
}

impl Element for Canvas {
    type RequestLayoutState = ();
    type PrepaintState = Placed;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Placed {
        let desk = self.desk.read(cx).size().unwrap_or_default();
        let (shown, fit) = fitted(bounds, desk);
        if self.view.read(cx).placed != Some((shown.origin, fit)) {
            self.view
                .update(cx, |view, _| view.placed = Some((shown.origin, fit)));
        }
        Placed {
            hitbox: window.insert_hitbox(shown, HitboxBehavior::Normal),
            shown,
            fit,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        placed: &mut Placed,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (shown, fit, p) = (placed.shown, placed.fit, self.palette);
        window.paint_quad(fill(shown, p.sunken));
        let key = ElementId::NamedInteger("agent-desktop".into(), self.view.entity_id().as_u64());
        let owner = self.desk.downgrade();
        let _shown = window.use_keyed_state(key, cx, |window, cx| Shown::new(owner, window, cx));
        {
            let desk = self.desk.read(cx);
            let picture = desk.picture.read(cx);
            let mask = Some(ContentMask { bounds: shown });
            for listed in desk.backdrop.iter().chain(desk.windows.iter().rev()) {
                let frame = listed.frame;
                let at = shown.origin + point(px(frame.x as f32 * fit), px(frame.y as f32 * fit));
                let area = size(px(frame.width as f32 * fit), px(frame.height as f32 * fit));
                let bounds = Bounds::new(at, area);
                window.with_content_mask(mask, |window| {
                    match picture.frame_of(listed.id) {
                        Some(pixels) => window.paint_surface(bounds, pixels.clone()),
                        None => window.paint_quad(fill(bounds, p.card)),
                    }
                });
            }
        }
        if self.focused {
            let focus = self.view.read(cx).focus.clone();
            window.handle_input(
                &focus,
                ElementInputHandler::new(shown, self.view.clone()),
                cx,
            );
        }
        self.listen(&placed.hitbox, window);
    }
}

impl Canvas {
    /// The pointer over the desktop: a press focuses the tab, a drag that started on it keeps
    /// reaching it, and what's scrolled over it scrolls the window under it, not the pane.
    fn listen(&self, hitbox: &Hitbox, window: &mut Window) {
        let (down, over_down) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !over_down.is_hovered(window) {
                return;
            }
            let _ = down.update(cx, |view, cx| view.pressed_at(event, window, cx));
            cx.stop_propagation();
        });
        let up = self.view.downgrade();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = up.update(cx, |view, cx| view.released_at(event, cx));
            }
        });
        let (moved, over_move) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let over = over_move.is_hovered(window);
            let Some(moved) = moved.upgrade() else {
                return;
            };
            if over || moved.read(cx).pressed.is_some() {
                moved.update(cx, |view, cx| view.moved_to(event, over, cx));
            }
        });
        let (scrolled, over_scroll) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !over_scroll.should_handle_scroll(window) {
                return;
            }
            let _ = scrolled.update(cx, |view, cx| view.wheel(event, cx));
            cx.stop_propagation();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(10.), px(20.)), size(px(width), px(height)))
    }

    #[test]
    fn the_desktop_keeps_its_shape_and_is_centred() {
        let desk = koyo_desk_wire::Size {
            width: 2000.,
            height: 1000.,
        };
        let (shown, fit) = fitted(area(1200., 400.), desk);
        assert!((fit - 0.4).abs() < 1e-6);
        assert_eq!(shown.size, size(px(800.), px(400.)));
        assert_eq!(shown.origin, point(px(210.), px(20.)));
        let (shown, fit) = fitted(area(4000., 3000.), desk);
        assert!((fit - 1.).abs() < 1e-6);
        assert_eq!(shown.size, size(px(2000.), px(1000.)));
    }

    #[test]
    fn a_desktop_not_made_yet_takes_the_pane() {
        let (shown, fit) = fitted(area(300., 200.), koyo_desk_wire::Size::default());
        assert_eq!(shown, area(300., 200.));
        assert!((fit - 1.).abs() < 1e-6);
    }
}
