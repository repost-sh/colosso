//! The page itself, as an element: its latest frame drawn in the window's own frame, at the
//! pane's size (or, in responsive mode, at the size chosen, scaled to fit and centred), and
//! every pointer event over it. The page counts as on screen while the element is drawn with
//! no notice over it, frame or not (`Shown`, in its state): a pane that isn't drawn takes its
//! page out of sight, unless an agent is using it (`view/sight.rs`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, Context, DispatchPhase, Div, Element, ElementId, ElementInputHandler, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    PinchEvent, Pixels, ScrollEvent, Stateful, Style, Styled as _, Window, div, fill, relative,
    white,
};

use super::frames::Shown;
use super::input::{self, cursor_style};
use super::pointer;
use super::responsive::Grip;
use super::toolbar;
use super::view::BrowserView;
use super::view::lifecycle::{Dragging, MOVING};
use crate::responsive::{self, grips};
use crate::theme::{self, Palette};

pub struct PageSurface {
    pub view: Entity<BrowserView>,
    pub focused: bool,
    /// Whether a notice covers the page, which then isn't drawn, and so counts as out of
    /// sight: the host stops painting it.
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
        let (laid, responsive, free, dragging, placed) = {
            let view = self.view.read(cx);
            let mode = view.responsive.mode.as_ref();
            let laid = view.lay_out(bounds, scale);
            // What moves the pane's edge (`Dragging`): a drag in hand or a grip held, else
            // layout, live while its sizes keep coming (the window's edge, the sidebar folding).
            let moving = (view.resized_at).is_some_and(|at| at.elapsed() < MOVING);
            let dragging = if cx.has_active_drag() || view.responsive.drag.is_some() {
                Dragging::Held
            } else if moving {
                Dragging::Layout
            } else {
                Dragging::No
            };
            // A held drag let go is told even at the size it ended at.
            let placed = view.is_placed(&laid, bounds.size, scale)
                && (dragging == Dragging::Held || !view.drag_sent);
            let free = mode.is_some_and(|mode| mode.device.is_none());
            (laid, mode.is_some(), free, dragging, placed)
        };
        let (shown, fit) = (laid.1, laid.2);
        // An update counts as a change to the view, so it's made only when one happened.
        if !placed {
            self.view.update(cx, |view, cx| {
                view.place_page(laid, bounds.size, window, dragging, cx);
            });
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
        let (pane, frame, cursor, open) = {
            let view = self.view.read(cx);
            let frame = (view.frame.clone()).filter(|_| view.shows_frames());
            (view.pane, frame, view.cursor, view.is_open(cx))
        };
        // Drawn with nothing over it, the page is on screen whether or not a frame is in yet:
        // one opened out of sight, or hidden before it painted, paints once it's shown.
        if !self.covered {
            let key = ElementId::NamedInteger("browser-page-view".into(), pane);
            let owner = self.view.downgrade();
            let shown = window.use_keyed_state(key, cx, |_, cx| Shown::new(owner, cx));
            if let Some(frame) = frame {
                frame.paint(bounds, placed.fit, window);
            }
            let shown = shown.downgrade();
            let view = self.view.read(cx);
            // An update counts as a change to the view, so it's made only when one happened.
            if view.shown.as_ref() != Some(&shown) || view.visible != Some(true) {
                self.view.update(cx, |view, cx| {
                    view.shown = Some(shown);
                    view.tell_sight(cx);
                });
            }
        }
        if open {
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
        self.listen(hitbox, window);
    }
}

impl PageSurface {
    /// The pointer over the page: presses focus it, a drag that started in it keeps
    /// reaching it, and what's scrolled or pinched over it scrolls or zooms it rather than
    /// the pane.
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
        let (pinched, hitbox_pinch) = (self.view.downgrade(), hitbox.clone());
        window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hitbox_pinch.is_hovered(window) {
                return;
            }
            let _ = pinched.update(cx, |view, cx| view.pinch(event, cx));
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
        window: &mut Window,
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
        let bar = toolbar::control_bar(self, window, cx);
        // Under the bar, which names the agent the marks show at work.
        let marks = pointer::overlay(self, window, cx);
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
            .children(marks)
            .children(bar)
            .children(overlay)
            .children(self.responsive.mode.as_ref().and_then(|mode| {
                grips::label(self.responsive.drag.as_ref(), mode.width, mode.height, p)
            }));
        input::listen(page, cx)
    }
}
