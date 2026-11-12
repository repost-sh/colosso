//! The device's picture as an element (`docs/devices.md` 8): fitted to the pane and centred,
//! never larger than one of the device's pixels to one of the screen's, its corners rounded as
//! the device's own screen's are, its frames painted as they come, and the pointer over it
//! heard. The device counts as drawn while the element is
//! (`Shown`, in its state): the host keeps the picture current only then, or for an agent.

use gpui_kit::{
    App, Bounds, ContentMask, Context, Corners, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Overflow, Pixels, ScrollEvent, Style, WeakEntity, Window, point, px, relative, size,
};
use koyo_device_wire::Size;

use super::link::Link;
use super::view::DeviceView;

pub struct Canvas {
    pub view: Entity<DeviceView>,
    pub link: Entity<Link>,
    pub focused: bool,
}

pub struct Placed {
    hitbox: Hitbox,
    shown: Bounds<Pixels>,
    /// The radius of the screen's corners as shown.
    corner: Pixels,
}

/// The device drawn in a window, kept by the element for as long as it's drawn.
pub struct Shown;

impl Shown {
    fn new(link: WeakEntity<Link>, cx: &mut Context<Self>) -> Self {
        let me = cx.entity_id();
        let _ = link.update(cx, |link, _| link.set_shown_by(me, true));
        cx.on_release(move |_: &mut Self, cx| {
            let _ = link.update(cx, |link, _| link.set_shown_by(me, false));
        })
        .detach();
        Shown
    }
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

impl IntoElement for Canvas {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
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
        let screen = self.link.read(cx).screen();
        let (points, scale, corner) = screen.map_or((Size::default(), 1., 0.), |s| (s.size, s.scale as f32, s.corner as f32));
        // Never larger than one device pixel to one of the screen's.
        let most = scale / window.scale_factor().max(1.);
        let (shown, fit) = fitted(bounds, points, most);
        if self.view.read(cx).placed != Some((shown.origin, fit)) {
            self.view
                .update(cx, |view, _| view.placed = Some((shown.origin, fit)));
        }
        Placed {
            hitbox: window.insert_hitbox(shown, HitboxBehavior::Normal),
            shown,
            corner: px(corner * fit),
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
        let shown = placed.shown;
        let key = ElementId::NamedInteger("device".into(), self.view.entity_id().as_u64());
        let owner = self.link.downgrade();
        let _shown = window.use_keyed_state(key, cx, |_, cx| Shown::new(owner, cx));
        let picture = self.link.read(cx).picture.clone();
        let pixels = picture.read(cx).shown.as_ref().map(|shown| shown.pixels.clone());
        if let Some(pixels) = pixels {
            // A box of the screen's shape: its rounded corners clip the frame, as the device's do.
            let corner = gpui_kit::AbsoluteLength::from(placed.corner);
            let shape = Style {
                overflow: point(Overflow::Hidden, Overflow::Hidden),
                corner_radii: Corners {
                    top_left: corner,
                    top_right: corner,
                    bottom_right: corner,
                    bottom_left: corner,
                },
                ..Style::default()
            };
            shape.paint(shown, window, cx, |window, _| {
                window.with_content_mask(Some(ContentMask { bounds: shown }), |window| window.paint_surface(shown, pixels));
            });
            picture.update(cx, |picture, _| picture.paints += 1);
        }
        if self.focused {
            let focus = self.view.read(cx).focus.clone();
            window.handle_input(&focus, ElementInputHandler::new(shown, self.view.clone()), cx);
        }
        self.listen(&placed.hitbox, window);
    }
}

impl Canvas {
    /// The pointer over the picture: a press focuses the tab, a drag that began on it keeps
    /// reaching it, and what's scrolled over it scrolls the device, not the pane.
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
            let _ = moved.update(cx, |view, cx| view.moved_to(event, over, cx));
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
    fn a_phone_fits_a_short_pane_by_its_height_and_is_centred() {
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
    fn a_large_pane_shows_it_no_larger_than_its_pixels_allow() {
        let phone = Size {
            width: 402.,
            height: 874.,
        };
        let (_, fit) = fitted(area(4000., 4000.), phone, 1.5);
        assert!((fit - 1.5).abs() < 1e-6);
    }
}
