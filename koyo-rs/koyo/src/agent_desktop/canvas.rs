//! The desktop itself, as an element (`docs/agent-desktop.md` 9): the desktop's shape fitted
//! to the pane, each window's latest frame drawn at its place, back to front as the host
//! lists them, and every pointer event over it. The desktop counts as on screen while the
//! element is drawn (`Shown`, in its state) in a window seen: the host sends its frames only
//! then.

use gpui_kit::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, LayoutId, Pixels, Style, Window, fill, point, px,
    relative, size,
};

use super::Desk;
use super::view::DeskView;
use crate::hosted::canvas::{fitted, listen};
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
        let (shown, fit) = fitted(bounds, desk, 1.);
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
        listen(&self.view, &placed.hitbox, window);
    }
}
