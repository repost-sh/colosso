//! The element a viewer's pages are painted with: each page where the camera puts it, its
//! paper or a checkerboard under what may be see-through, then the picture, or the tiles drawn
//! of it, older ones first, each on the screen's own pixels. It hands the viewer the pointer's
//! wheel, pinches, presses and drags, and its size as it's laid out.

use std::sync::Arc;

use gpui_kit::{
    App, Bounds, ContentMask, Corners, CursorStyle, DispatchPhase, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, RenderImage, ScrollEvent,
    Style, Window, checkerboard, fill, outline, point, px, relative, size,
};

use super::canvas::{Camera, Fit, Rect, clamp, fit_zoom, layout, offset};
use super::tiles::TileKey;
use super::view::Viewer;

/// A checkerboard square's side, in logical pixels.
const SQUARE: f32 = 8.;

/// What a viewer paints, as of its last render.
pub struct Pages {
    pub view: Entity<Viewer>,
    pub sizes: Arc<[(f32, f32)]>,
    pub camera: Camera,
    /// How the pages fit the pane while the camera fits them, worked out again as it paints so
    /// a pane being resized is painted at its new size at once.
    pub fit: Fit,
    pub picture: Option<(Arc<RenderImage>, usize)>,
    /// Tiles, those to paint first first.
    pub tiles: Vec<(TileKey, Arc<RenderImage>)>,
    /// White paper under each page, with an edge in this colour.
    pub paper: Option<Hsla>,
    /// A checkerboard under what may be see-through, darker on a dark theme.
    pub checker: Option<bool>,
    pub dragging: bool,
}

impl IntoElement for Pages {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Pages {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

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
    ) -> Hitbox {
        let viewport = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let scale = window.scale_factor();
        let view = self.view.clone();
        if (view.read(cx).viewport, view.read(cx).scale) != (viewport, scale) {
            // Not while drawing: the viewer asks for what's in sight at the new size.
            cx.defer(move |cx| view.update(cx, |view, cx| view.set_viewport(viewport, scale, cx)));
        }
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        (): &mut (),
        hitbox: &mut Hitbox,
        window: &mut Window,
        _: &mut App,
    ) {
        self.listen(bounds, hitbox, window);
        let viewport = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let zoom = if self.camera.fit {
            fit_zoom(&self.sizes, viewport, self.fit)
        } else {
            self.camera.zoom
        };
        let laid = layout(&self.sizes, zoom);
        let scroll = clamp(self.camera.scroll, laid.size, viewport);
        let (left, top) = offset(laid.size, viewport, scroll);
        let pannable = laid.size.0 > viewport.0 + 0.5 || laid.size.1 > viewport.1 + 0.5;
        let cursor = if self.dragging {
            CursorStyle::ClosedHand
        } else if pannable {
            CursorStyle::OpenHand
        } else {
            CursorStyle::Arrow
        };
        window.set_cursor_style(cursor, hitbox);
        let scale = window.scale_factor();
        // Each page's place in the window, on the screen's pixels.
        let snap = |value: Pixels| px((f32::from(value) * scale).round() / scale);
        let placed: Vec<Bounds<Pixels>> = (laid.pages.iter())
            .map(|page| {
                let page = page.shifted((left, top));
                Bounds::new(
                    point(snap(bounds.origin.x + px(page.x)), snap(bounds.origin.y + px(page.y))),
                    size(px(page.width), px(page.height)),
                )
            })
            .collect();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for page in &placed {
                let seen = page.intersect(&bounds);
                if seen.is_empty() {
                    continue;
                }
                if let Some(edge) = self.paper {
                    window.paint_quad(fill(*page, gpui_kit::white()));
                    window.paint_quad(outline(page.dilate(px(1.)), edge, gpui_kit::BorderStyle::Solid));
                }
                if let Some(dark) = self.checker {
                    paint_checker(seen, dark, window);
                }
            }
            if let (Some((picture, frame)), Some(page)) = (&self.picture, placed.first()) {
                let seen = page.intersect(&bounds);
                window
                    .paint_image(seen, *page, Corners::default(), picture.clone(), *frame, false)
                    .ok();
            }
            for (key, image) in &self.tiles {
                let (Some(points), Some(page)) = (self.sizes.get(key.page), placed.get(key.page))
                else {
                    continue;
                };
                let on = Rect {
                    x: f32::from(page.origin.x),
                    y: f32::from(page.origin.y),
                    width: f32::from(page.size.width),
                    height: f32::from(page.size.height),
                };
                let shown = key.shown_at(*points, on, zoom);
                let shown = Bounds::new(
                    point(px(shown.x), px(shown.y)),
                    size(px(shown.width), px(shown.height)),
                );
                let seen = shown.intersect(page).intersect(&bounds);
                if !seen.is_empty() {
                    window.paint_image(seen, shown, Corners::default(), image.clone(), 0, false).ok();
                }
            }
        });
    }
}

impl Pages {
    /// The pointer goes to the viewer: the wheel and pinches over the pages, presses on them,
    /// and moves and releases anywhere, so a drag that leaves the pane keeps going.
    fn listen(&self, bounds: Bounds<Pixels>, hitbox: &Hitbox, window: &mut Window) {
        let origin = bounds.origin;
        let (view, hit) = (self.view.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hit.should_handle_scroll(window) {
                view.update(cx, |view, cx| view.wheel(event, origin, cx));
                cx.stop_propagation();
            }
        });
        let (view, hit) = (self.view.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hit.is_hovered(window) {
                view.update(cx, |view, cx| view.pinch(event, origin, cx));
                cx.stop_propagation();
            }
        });
        let (view, hit) = (self.view.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            let left = event.button == gpui_kit::MouseButton::Left;
            if phase == DispatchPhase::Bubble && left && hit.is_hovered(window) {
                view.update(cx, |view, cx| view.press(event, origin, window, cx));
                cx.stop_propagation();
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && view.read(cx).drag.is_some() {
                view.update(cx, |view, cx| view.drag_to(event, origin, cx));
            }
        });
        let view = self.view.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && view.read(cx).drag.is_some() {
                view.update(cx, |view, cx| {
                    view.drag = None;
                    cx.notify();
                });
            }
        });
    }
}

/// A checkerboard over `area`: a light grey, with a darker one in every other square, the
/// squares on the screen's pixels and fixed in the window as the pages move under them.
fn paint_checker(area: Bounds<Pixels>, dark: bool, window: &mut Window) {
    let (one, other) = if dark { (0x26, 0x31) } else { (0xff, 0xe9) };
    let grey = |shade: u32| gpui_kit::rgb(shade * 0x01_01_01);
    // The pattern starts at its quad's corner: one on a pair of squares' corner in the window.
    let pair = 2. * SQUARE;
    let corner = |from: Pixels| px((f32::from(from) / pair).floor() * pair);
    let from = point(corner(area.origin.x), corner(area.origin.y));
    let pattern = Bounds::from_corners(from, area.bottom_right());
    window.paint_quad(fill(area, grey(one)));
    let squares = checkerboard(grey(other), SQUARE * window.scale_factor());
    window.with_content_mask(Some(ContentMask { bounds: area }), |window| {
        window.paint_quad(fill(pattern, squares));
    });
}
