//! Boxes anchored at the caret, placed from the input's current-frame geometry.
//!
//! The input stashes fresh caret geometry in its prepaint
//! ([`PrepaintCaretGeometry`]). The boxes live inside `deferred()`, so their
//! prepaint runs after the input's prepaint but before its paint; resolving
//! the anchor there puts them at the current frame's caret instead of the
//! previous frame's.
//!
//! The first box goes under the caret's line, or over it when there's no
//! room under; each next box goes to the right of the one before while the
//! window has room, else under it, or over it when the first is over the
//! caret. Each is framed as the editor's cards are, over the same backdrop.

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, DispatchPhase, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseDownEvent, Pixels, Point, Size, Style,
    WeakEntity, Window, point, px,
};

use crate::input::{EditorState, PrepaintCaretGeometry};

use super::{GAP, RADIUS, SNAP_TO_EDGE, cards::popover_backdrop};

/// Builds one box, told whether a backdrop shows behind it.
pub(crate) type BoxBuilder = Box<dyn Fn(bool, &mut Window, &mut App) -> AnyElement>;

pub(crate) struct CaretAnchoredPopup {
    editor: WeakEntity<EditorState>,
    boxes: Vec<BoxBuilder>,
    /// Called on a mouse down outside every box.
    on_click_out: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    placed: Vec<Bounds<Pixels>>,
    content: Vec<AnyElement>,
}

impl CaretAnchoredPopup {
    pub(crate) fn new(editor: WeakEntity<EditorState>, boxes: Vec<BoxBuilder>) -> Self {
        Self {
            editor,
            boxes,
            on_click_out: None,
            placed: Vec::new(),
            content: Vec::new(),
        }
    }

    /// Has `handler` called on a mouse down outside every box.
    pub(crate) fn on_click_out(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click_out = Some(Box::new(handler));
        self
    }
}

/// The caret's line in window coordinates, where the caret itself is painted.
fn caret_line(geometry: PrepaintCaretGeometry) -> Bounds<Pixels> {
    let bounds = geometry.cursor_bounds();
    Bounds::new(
        point(bounds.origin.x, bounds.origin.y + geometry.scroll_offset().y),
        Size::new(bounds.size.width, geometry.line_height()),
    )
}

/// Where a box of `size` goes, the boxes `before` it placed already.
fn placement(
    size: Size<Pixels>,
    before: &[Bounds<Pixels>],
    caret: Bounds<Pixels>,
    viewport: Size<Pixels>,
) -> Point<Pixels> {
    let Some((first, last)) = before.first().zip(before.last()) else {
        let x = (caret.left() - px(4.))
            .min(viewport.width - size.width - SNAP_TO_EDGE)
            .max(SNAP_TO_EDGE);
        let below = caret.bottom() + GAP;
        let above = caret.top() - GAP - size.height;
        let fits_below = below + size.height <= viewport.height - SNAP_TO_EDGE;
        let y = if fits_below || above < SNAP_TO_EDGE { below } else { above };
        return point(x, y);
    };
    let over = first.top() < caret.top();
    let x = last.right() + GAP;
    if x + size.width <= viewport.width - SNAP_TO_EDGE {
        // Beside the last: top-aligned under the caret, bottom-aligned over it.
        let y = if over { last.bottom() - size.height } else { last.top() };
        point(x, y.max(SNAP_TO_EDGE))
    } else if over {
        point(first.left(), last.top() - GAP - size.height)
    } else {
        point(first.left(), last.bottom() + GAP)
    }
}

impl Element for CaretAnchoredPopup {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // The real layout happens in prepaint, once the anchor is resolved.
        let layout_id = window.request_layout(Style::default(), [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        let Some(geometry) = editor.read(cx).prepaint_caret_geometry() else {
            return;
        };
        let caret = caret_line(geometry);
        let viewport = window.viewport_size();
        let backdrop = popover_backdrop(cx);
        self.placed.clear();
        self.content.clear();
        for (i, build) in self.boxes.iter().enumerate() {
            window.with_id(ElementId::from(("caret-box", i)), |window| {
                let shows = backdrop
                    .as_ref()
                    .is_some_and(|backdrop| backdrop.shows(window, cx));
                let mut content = build(shows, window, cx);
                let size = content.layout_as_root(
                    Size {
                        width: AvailableSpace::Definite(viewport.width),
                        height: AvailableSpace::Definite(viewport.height),
                    },
                    window,
                    cx,
                );
                let origin = placement(size, &self.placed, caret, viewport);
                let bounds = Bounds::new(origin, size);
                if let Some(backdrop) = backdrop.as_ref().filter(|_| shows) {
                    backdrop.place(bounds, RADIUS, window, cx);
                }
                content.prepaint_at(origin, window, cx);
                self.placed.push(bounds);
                self.content.push(content);
            });
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (i, content) in self.content.iter_mut().enumerate() {
            window.with_id(ElementId::from(("caret-box", i)), |window| {
                content.paint(window, cx);
            });
        }
        if let Some(on_click_out) = self.on_click_out.take() {
            let placed = std::mem::take(&mut self.placed);
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && !placed.iter().any(|bounds| bounds.contains(&event.position))
                {
                    on_click_out(window, cx);
                }
            });
        }
    }
}

impl IntoElement for CaretAnchoredPopup {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    fn bounds(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    #[test]
    fn the_first_box_goes_under_the_caret_or_over_it_when_there_is_no_room() {
        let viewport = size(px(800.), px(600.));
        let caret = bounds(100., 100., 2., 20.);
        let at = placement(size(px(200.), px(240.)), &[], caret, viewport);
        assert_eq!(at, point(px(96.), px(124.)));
        // No room under a caret near the bottom: over its line instead.
        let caret = bounds(100., 500., 2., 20.);
        let at = placement(size(px(200.), px(240.)), &[], caret, viewport);
        assert_eq!(at, point(px(96.), px(256.)));
        // Nor over it: under, as it was.
        let at = placement(size(px(200.), px(500.)), &[], caret, viewport);
        assert_eq!(at, point(px(96.), px(524.)));
        // Kept off the window's right edge.
        let caret = bounds(700., 100., 2., 20.);
        let at = placement(size(px(200.), px(240.)), &[], caret, viewport);
        assert_eq!(at, point(px(592.), px(124.)));
    }

    #[test]
    fn the_next_box_goes_beside_the_last_or_under_it_when_there_is_no_room() {
        let viewport = size(px(800.), px(600.));
        let caret = bounds(100., 100., 2., 20.);
        let first = bounds(96., 124., 200., 240.);
        let at = placement(size(px(300.), px(100.)), &[first], caret, viewport);
        assert_eq!(at, point(px(300.), px(124.)));
        // Beside a box over the caret, its bottom aligned.
        let over = bounds(96., 20., 200., 60.);
        let at = placement(size(px(300.), px(40.)), &[over], caret, viewport);
        assert_eq!(at, point(px(300.), px(40.)));
        // No room to the right: under the first, or over one that's over.
        let at = placement(size(px(600.), px(100.)), &[first], caret, viewport);
        assert_eq!(at, point(px(96.), px(368.)));
        let at = placement(size(px(600.), px(10.)), &[over], caret, viewport);
        assert_eq!(at, point(px(96.), px(6.)));
    }
}
