//! What moves a viewer's camera: a trackpad's pinch and ⌘ with the wheel zoom about the
//! pointer; the wheel, a trackpad's two fingers and a drag move about; a double click goes
//! between fitting and the file's own size; and the keys zoom and move as a browser's do.

use gpui_kit::{
    Context, MouseDownEvent, MouseMoveEvent, PinchEvent, Pixels, Point, ScrollEvent, Window,
    actions, px,
};

use super::canvas::{clamp, layout, step, zoom_about};
use super::view::Viewer;

actions!(
    viewer,
    [
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        ActualSize,
        ScrollUp,
        ScrollDown,
        ScrollLeft,
        ScrollRight,
        PageUp,
        PageDown,
        Home,
        End
    ]
);

/// How far an arrow key moves, in logical pixels.
const ARROW: f32 = 40.;
/// How far a line of a mouse's wheel moves, in logical pixels.
const WHEEL_LINE: f32 = 40.;
/// How far ⌘ and the wheel go to zoom in twice as far: a notch of a mouse's wheel zooms about
/// 15%, a trackpad's two fingers as far as they go.
const DOUBLING: f32 = 200.;

impl Viewer {
    /// Zooms to `zoom` about `anchor`, a point of the pane, or its middle.
    pub(super) fn zoom_to(&mut self, zoom: f32, anchor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        if self.pages.is_empty() {
            return;
        }
        let anchor = anchor.unwrap_or((self.viewport.0 / 2., self.viewport.1 / 2.));
        self.camera = zoom_about(&self.pages, self.viewport, self.camera, zoom, anchor);
        self.camera_moved(true, cx);
    }

    /// Moves the pages by `by` logical pixels, as far as they go.
    pub(super) fn scroll_by(&mut self, by: (f32, f32), cx: &mut Context<Self>) {
        let laid = layout(&self.pages, self.camera.zoom);
        let scroll = (self.camera.scroll.0 + by.0, self.camera.scroll.1 + by.1);
        let scroll = clamp(scroll, laid.size, self.viewport);
        if scroll != self.camera.scroll {
            self.camera.scroll = scroll;
            self.camera_moved(false, cx);
        }
    }

    /// Fits the pages to the pane again, and follows it as it's resized.
    pub(super) fn fit(&mut self, cx: &mut Context<Self>) {
        self.camera.fit = true;
        self.refit();
        self.camera_moved(true, cx);
    }

    /// The wheel over the pages, `origin` being the pane's top left corner in the window.
    pub(super) fn wheel(&mut self, event: &ScrollEvent, origin: Point<Pixels>, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(WHEEL_LINE));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if event.modifiers.platform {
            let anchor = local(event.position, origin);
            self.zoom_to(self.camera.zoom * 2f32.powf(dy / DOUBLING), Some(anchor), cx);
        } else {
            // The wheel's deltas say where the pages go; the scroll is how far they've gone.
            self.scroll_by((-dx, -dy), cx);
        }
    }

    pub(super) fn pinch(&mut self, event: &PinchEvent, origin: Point<Pixels>, cx: &mut Context<Self>) {
        let anchor = local(event.position, origin);
        self.zoom_to(self.camera.zoom * (1. + event.delta), Some(anchor), cx);
    }

    /// A press: the keyboard comes here, a double click goes between fitting and the file's own
    /// size about the pointer, and a drag starts.
    pub(super) fn press(
        &mut self,
        event: &MouseDownEvent,
        origin: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let at = local(event.position, origin);
        if event.click_count == 2 {
            self.drag = None;
            if self.camera.fit && (self.camera.zoom - 1.).abs() > 0.01 {
                self.zoom_to(1., Some(at), cx);
            } else {
                self.fit(cx);
            }
            return;
        }
        self.drag = Some(at);
    }

    /// The pointer moved: while dragging, the pages follow it.
    pub(super) fn drag_to(&mut self, event: &MouseMoveEvent, origin: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(from) = self.drag else {
            return;
        };
        if event.pressed_button.is_none() {
            self.drag = None;
            return;
        }
        let at = local(event.position, origin);
        self.drag = Some(at);
        self.scroll_by((from.0 - at.0, from.1 - at.1), cx);
    }

    pub(super) fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(step(self.camera.zoom, true), None, cx);
    }

    pub(super) fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(step(self.camera.zoom, false), None, cx);
    }

    pub(super) fn zoom_to_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.fit(cx);
    }

    pub(super) fn actual_size(&mut self, _: &ActualSize, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(1., None, cx);
    }

    pub(super) fn scroll_up(&mut self, _: &ScrollUp, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., -ARROW), cx);
    }

    pub(super) fn scroll_down(&mut self, _: &ScrollDown, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., ARROW), cx);
    }

    pub(super) fn scroll_left(&mut self, _: &ScrollLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((-ARROW, 0.), cx);
    }

    pub(super) fn scroll_right(&mut self, _: &ScrollRight, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((ARROW, 0.), cx);
    }

    pub(super) fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., -self.viewport.1 * 0.9), cx);
    }

    pub(super) fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., self.viewport.1 * 0.9), cx);
    }

    pub(super) fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., -self.camera.scroll.1), cx);
    }

    pub(super) fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.scroll_by((0., f32::MAX / 4.), cx);
    }
}

/// `position` in the window as a point of the pane whose top left corner is at `origin`.
fn local(position: Point<Pixels>, origin: Point<Pixels>) -> (f32, f32) {
    (f32::from(position.x - origin.x), f32::from(position.y - origin.y))
}
