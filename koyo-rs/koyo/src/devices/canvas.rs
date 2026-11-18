//! The device's picture as an element (`docs/devices.md` 8): fitted to the pane and centred,
//! never larger than one of the device's pixels to one of the screen's, its corners rounded as
//! the device's own screen's are, its frames painted as they come, and the pointer over it
//! heard. The device counts as drawn while the element is (`Shown`, in its state), in a window
//! seen: the host keeps the picture current only then, or for an agent.

use gpui_kit::{
    App, Bounds, ContentMask, Corners, Element, ElementId, ElementInputHandler, Entity, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, LayoutId, Overflow, Pixels, Style, Window, point, px,
    relative,
};
use koyo_device_wire::Size;

use super::link::Link;
use super::view::DeviceView;
use crate::hosted::canvas::{fitted, listen};
use crate::sight::Shown;

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
        let _shown = window.use_keyed_state(key, cx, |window, cx| Shown::new(owner, window, cx));
        let picture = self.link.read(cx).picture.clone();
        let pixels = picture.read(cx).shown.as_ref().map(|shown| shown.pixels.clone());
        if let Some(pixels) = pixels {
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
        listen(&self.view, &placed.hitbox, window);
    }
}
