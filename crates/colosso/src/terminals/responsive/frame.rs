//! What's drawn round a pinned terminal, over its area: the outline round the grid as the
//! terminal last laid it out, and its grips while it's a free size, as round a page.

use gpui_kit::{
    App, Bounds, Element, ElementId, Entity, GlobalElementId, Hitbox, InspectorElementId,
    IntoElement, LayoutId, Pixels, Position, Style, Window, px, relative,
};

use super::ResponsiveTerminal;
use crate::responsive::{self, Grip, grips};
use crate::theme;

/// Laid over the whole area, after the terminal, so the grid's bounds are this frame's.
pub struct Frame {
    pub host: Entity<ResponsiveTerminal>,
    /// Whether the grips show: a free size, not a preset.
    pub free: bool,
}

impl IntoElement for Frame {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Frame {
    type RequestLayoutState = ();
    type PrepaintState = Option<(Bounds<Pixels>, Vec<(Grip, Hitbox)>)>;

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
        let mut style = Style {
            position: Position::Absolute,
            ..Style::default()
        };
        style.inset.top = px(0.).into();
        style.inset.left = px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let (shown, _) = self.host.read(cx).view.read(cx).pinned_grid()?;
        let grips = if self.free {
            grips::hitboxes(shown, window)
        } else {
            Vec::new()
        };
        Some((shown, grips))
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut (),
        laid: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((shown, grips)) = laid else {
            return;
        };
        let p = theme::palette(cx);
        responsive::paint_outline(*shown, p, window);
        let held = self.host.read(cx).drag.as_ref().map(|drag| drag.grip);
        grips::paint(grips, held, &self.host, p, window);
    }
}
