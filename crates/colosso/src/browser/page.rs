//! The page itself, as an element: its latest frame in a native view over the pane, at the
//! pane's size, and every pointer event over it. The native view lives in the element's
//! state, so a pane that isn't drawn takes its page out of the window.

use colosso_browser_ipc::Viewport;
use gpui_kit::{
    App, Bounds, DispatchPhase, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, Style, Window, relative,
};

use super::frames::Shown;
use super::input::cursor_style;
use super::view::BrowserView;

pub struct PageSurface {
    pub view: Entity<BrowserView>,
    pub focused: bool,
    /// Whether a notice covers the page, which then isn't shown: drawn over the page's
    /// native layer, the notice would be under it.
    pub covered: bool,
}

impl IntoElement for PageSurface {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for PageSurface {
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
        let viewport = Viewport {
            width: bounds.size.width.as_f32().round().max(1.) as u32,
            height: bounds.size.height.as_f32().round().max(1.) as u32,
            scale: window.scale_factor(),
        };
        // An update counts as a change to the view, so it's made only when one happened.
        let placed = {
            let view = self.view.read(cx);
            view.origin == bounds.origin && view.viewport == Some(viewport)
        };
        if !placed {
            self.view.update(cx, |view, cx| {
                view.origin = bounds.origin;
                view.viewport_changed(viewport, cx);
            });
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
        cx: &mut App,
    ) {
        let (pane, frame, cursor, open, hovered) = {
            let view = self.view.read(cx);
            let frame = (view.frame.clone()).filter(|_| view.shows_frames() && !self.covered);
            (
                view.pane,
                frame,
                view.cursor,
                view.is_open(cx),
                view.hovered,
            )
        };
        // A page in a native view gets the pointer from it, GPUI's hit testing stopping there.
        let mut native = false;
        if let Some(frame) = frame {
            let key = ElementId::NamedInteger("browser-page-view".into(), pane);
            let owner = self.view.downgrade();
            let shown = window.use_keyed_state(key, cx, |window, cx| Shown::new(owner, window, cx));
            native = shown.update(cx, |shown, _| {
                shown.show(&frame);
                shown.place(bounds, hitbox.content_mask.bounds, window);
                shown.takes_pointer()
            });
            let layer = shown.downgrade();
            let known = {
                let view = self.view.read(cx);
                view.shown.as_ref() == Some(&layer) && view.visible == Some(true)
            };
            if !known {
                self.view.update(cx, |view, cx| {
                    view.shown = Some(layer);
                    view.set_visible(true, cx);
                });
            }
        }
        if open && native {
            if hovered {
                window.set_window_cursor_style(cursor_style(cursor));
            }
        } else if open {
            window.set_cursor_style(cursor_style(cursor), hitbox);
        }
        if self.focused {
            let focus = self.view.read(cx).focus.clone();
            window.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.view.clone()),
                cx,
            );
        }
        if !native {
            self.listen(hitbox, window);
        }
    }
}

impl PageSurface {
    /// The pointer over the page: presses focus it, a drag that started in it keeps
    /// reaching it, and what's scrolled over it scrolls it rather than the pane.
    fn listen(&self, hitbox: &Hitbox, window: &mut Window) {
        let (down, hitbox_down) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox_down.is_hovered(window) {
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
        let (moved, hitbox_move) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let over = hitbox_move.is_hovered(window);
            let Some(moved) = moved.upgrade() else {
                return;
            };
            // Moves elsewhere in the window are nothing to this page.
            let view = moved.read(cx);
            if over || view.hovered || view.pressed.is_some() {
                moved.update(cx, |view, cx| view.moved_to(event, over, cx));
            }
        });
        let (scrolled, hitbox_scroll) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox_scroll.should_handle_scroll(window) {
                return;
            }
            let _ = scrolled.update(cx, |view, cx| view.wheel(event, cx));
            cx.stop_propagation();
        });
    }
}
