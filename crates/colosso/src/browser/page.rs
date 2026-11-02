//! The page itself, as an element: its latest frame in a native view over the pane, at the
//! pane's size (or, in responsive mode, at the size chosen, scaled to fit and centred), and
//! every pointer event over it. The native view lives in the element's state, so a pane that
//! isn't drawn takes its page out of the window.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, Context, DispatchPhase, Div, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, ScrollEvent, Stateful, Style, Styled as _, Window, div, fill, relative, white,
};

use super::frames::Shown;
use super::input::{self, cursor_style};
use super::responsive::Grip;
use super::toolbar;
use super::view::BrowserView;
use crate::responsive::{self, grips};
use crate::theme::{self, Palette};

pub struct PageSurface {
    pub view: Entity<BrowserView>,
    pub focused: bool,
    /// Whether a notice covers the page, which then isn't shown: drawn over the page's
    /// native layer, the notice would be under it.
    pub covered: bool,
    /// What a page in responsive mode shows on before its frame is in, and its outline.
    pub blank: bool,
    pub palette: Palette,
}

/// Where the page shows in the element, and what's there to point at.
pub struct Placed {
    hitbox: Hitbox,
    /// The page's bounds in the window: the element's, or in responsive mode the page's
    /// scaled and centred in it.
    shown: Bounds<Pixels>,
    /// How much of its true size the page shows at.
    fit: f32,
    responsive: bool,
    grips: Vec<(Grip, Hitbox)>,
}

impl IntoElement for PageSurface {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for PageSurface {
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
        let scale = window.scale_factor();
        let (laid, responsive, free) = {
            let view = self.view.read(cx);
            let mode = view.responsive.mode.as_ref();
            (
                view.lay_out(bounds, scale),
                mode.is_some(),
                mode.is_some_and(|mode| mode.device.is_none()),
            )
        };
        let (shown, fit) = (laid.1, laid.2);
        // An update counts as a change to the view, so it's made only when one happened.
        if !self.view.read(cx).is_placed(&laid, bounds.size, scale) {
            self.view
                .update(cx, |view, cx| view.place_page(laid, bounds.size, scale, cx));
        }
        // A new size, sent from here or since the last frame, shows with the page laid out
        // to it.
        if self.view.read(cx).frame_due {
            super::view::sized::await_frame(&self.view, cx);
        }
        let hitbox = window.insert_hitbox(shown, HitboxBehavior::Normal);
        let grips = if free {
            grips::hitboxes(shown, window)
        } else {
            Vec::new()
        };
        Placed {
            hitbox,
            shown,
            fit,
            responsive,
            grips,
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
        let (hitbox, bounds) = (&placed.hitbox, placed.shown);
        if placed.responsive {
            let p = self.palette;
            let page = if self.blank { p.card } else { white() };
            responsive::paint_outline(bounds, p, window);
            window.paint_quad(fill(bounds, page));
            let held = (self.view.read(cx).responsive.drag.as_ref()).map(|drag| drag.grip);
            grips::paint(&placed.grips, held, &self.view, p, window);
        }
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
                shown.place(bounds, hitbox.content_mask.bounds, placed.fit, window);
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

impl BrowserView {
    /// The page itself under its notices, taking the keys bound to it. In responsive mode
    /// it shows on the sunken area round it, with the size by the pointer while a grip is in
    /// hand.
    pub(super) fn page_element(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = theme::palette(cx);
        let status = self.status(cx);
        let start = self.is_start_page();
        let inspector = self.is_inspector();
        let responsive = self.responsive.mode.is_some();
        // The pane's own colour until the page's first frame is in, so a page never opens on
        // a blank white one; DevTools keep it.
        let blank = self.frame.is_none() || inspector;
        let overlay = toolbar::overlay(self, &status, cx);
        let bar = toolbar::control_bar(self, cx);
        let page = div()
            .id(ElementId::NamedInteger("browser-page".into(), self.pane))
            .key_context("BrowserPage")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(match (responsive, blank) {
                (true, _) => p.sunken,
                (false, true) => p.card,
                (false, false) => white(),
            })
            .child(PageSurface {
                view: cx.entity(),
                focused: self.focus.is_focused(window),
                covered: overlay.is_some(),
                blank,
                palette: p,
            })
            .when(start, |page| page.child(toolbar::start_page(p)))
            .children(bar)
            .children(overlay)
            .children(self.responsive.mode.as_ref().and_then(|mode| {
                grips::label(self.responsive.drag.as_ref(), mode.width, mode.height, p)
            }));
        input::listen(page, cx)
    }
}
