//! The page itself, as an element: its views placed in a native surface of the window's
//! composition, at the pane's size (or, in responsive mode, at the size chosen, scaled to fit
//! and centred), and a press on it heard on its way there. The page counts as on screen while
//! the element is drawn with no notice over it, painted yet or not (`Shown`, in its state): a
//! pane that isn't drawn takes its page out of sight, unless an agent is using it
//! (`view/sight.rs`).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Bounds, Context, DispatchPhase, Div, Element, ElementId, Entity, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement as _, IntoElement, LayoutId,
    MouseDownEvent, ParentElement as _, Pixels, Stateful, Style, Styled as _, Window, div, fill,
    relative, white,
};

use super::input::{self, cursor_style};
use super::pointer;
use super::responsive::Grip;
use super::toolbar;
use super::view::BrowserView;
use super::view::sight::Shown;
use crate::responsive::{self, grips};
use crate::theme::{self, Palette};

pub struct PageSurface {
    pub view: Entity<BrowserView>,
    /// Whether a notice covers the page, which then isn't drawn, and so counts as out of
    /// sight: the engine stops painting it.
    pub covered: bool,
    /// What a page in responsive mode shows on before it has said how it's painted, and its
    /// outline.
    pub blank: bool,
    pub palette: Palette,
}

/// Where the page shows in the element, and what's there to point at.
pub struct Placed {
    hitbox: Hitbox,
    /// The page's bounds in the window: the element's, or in responsive mode the page's
    /// scaled and centred in it.
    shown: Bounds<Pixels>,
    responsive: bool,
    grips: Vec<(Grip, Hitbox)>,
    /// The page's pane, and whether the host has the page open.
    pane: koyo_browser_ipc::PaneId,
    open: bool,
    /// The page's native view, placed there, while the page is open with no notice over it.
    #[cfg(all(target_os = "macos", not(test)))]
    native: Option<Entity<crate::native::surface::Kept<super::native::NativePage>>>,
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
        let (laid, responsive, free, placed, pane, open) = {
            let view = self.view.read(cx);
            let mode = view.responsive.mode.as_ref();
            let laid = view.lay_out(bounds, scale);
            let placed = view.is_placed(&laid, bounds.size, scale);
            let free = mode.is_some_and(|mode| mode.device.is_none());
            (laid, mode.is_some(), free, placed, view.pane, view.is_open(cx))
        };
        let shown = laid.1;
        // An update counts as a change to the view, so it's made only when one happened.
        if !placed {
            self.view.update(cx, |view, cx| {
                view.place_page(laid, bounds.size, window, cx);
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
            responsive,
            grips,
            pane,
            open,
            #[cfg(all(target_os = "macos", not(test)))]
            native: (open && !self.covered)
                .then(|| self.place_native(pane, shown, window, cx)),
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
        let (pane, open) = (placed.pane, placed.open);
        let cursor = self.view.read(cx).cursor;
        // A drag that resizes the page (a split's divider, a panel's edge) moves its view
        // against what GPUI draws beside it: from the first frame that did, each move of the
        // drag is drawn at once, in the transaction the view moves in, so the edge and the
        // page land together (`docs/native-pages.md` 6).
        #[cfg(all(target_os = "macos", not(test)))]
        if let Some(native) = placed.native.as_ref().map(Entity::downgrade) {
            window.on_mouse_event(move |_: &gpui_kit::MouseMoveEvent, phase, window, cx| {
                let resizing = phase == DispatchPhase::Capture
                    && cx.has_active_drag()
                    && (native.upgrade()).is_some_and(|native| {
                        (native.read(cx).0.as_ref())
                            .is_some_and(super::native::NativePage::resizing)
                    });
                if resizing {
                    crate::native::window::draw_in_transaction(window, cx);
                }
            });
        }
        // Drawn with nothing over it, the page is on screen whether or not it has painted yet:
        // one opened out of sight, or hidden before it painted, paints once it's shown.
        if !self.covered {
            let key = ElementId::NamedInteger("browser-page-view".into(), pane);
            let owner = self.view.downgrade();
            let shown = window.use_keyed_state(key, cx, |_, cx| Shown::new(owner, cx));
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
        // The page's pointer, keys and text are Chromium's own, handed to it by `AppKit`. GPUI
        // sets the window's cursor after each of its frames from what's under the pointer: over
        // a page, the page's own, which Chromium sets as well.
        if open {
            window.set_cursor_style(cursor_style(cursor), hitbox);
        }
        self.listen(bounds, window);
    }
}

impl PageSurface {
    /// The native view of `pane`'s page put over `shown`, for a page that's open with no notice
    /// over it. As the element is prepainted, with its hitbox, not as it's painted: once a frame
    /// is prepainted GPUI notes where the window's native views are, and what of its own a
    /// press finds is what none of them covers. A view moved while the frame is painted
    /// is noted where it was a frame before: after a drag that moved a pane's edge fast,
    /// the last frame's move a divider's width or more, the divider lay where GPUI still
    /// took the page to be, and a press on it picked nothing up until something else had
    /// the window drawn again (`docs/native-pages.md` 6).
    #[cfg(all(target_os = "macos", not(test)))]
    fn place_native(
        &self,
        pane: koyo_browser_ipc::PaneId,
        shown: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<crate::native::surface::Kept<super::native::NativePage>> {
        let clip = window.content_mask().bounds;
        let key = ElementId::NamedInteger("browser-native-page".into(), pane);
        let kept = crate::native::surface::keep(key, window, cx, |window| {
            super::native::NativePage::new(window, pane)
                .inspect_err(|why| log::error!("no place for a page: {why}"))
                .ok()
        });
        // A page the keyboard is wanted in takes it as its views come into the window
        // (`docs/keyboard.md`).
        if let Some(native) = kept.read(cx).0.as_ref() {
            native.place(shown, clip, window);
        }
        kept
    }

    /// A press on the page, which `AppKit` hands the page itself: GPUI's focus follows it into
    /// the page, so Koyo's keys are the page's, and the person has the page. Heard before the
    /// page hears it, and left to it.
    fn listen(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let down = self.view.downgrade();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture && bounds.contains(&event.position) {
                let _ = down.update(cx, |view, cx| view.pressed_at(event, window, cx));
            }
        });
    }
}

impl BrowserView {
    /// The page itself under its notices, taking the edits bound to it. In responsive mode
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
        // The pane's own colour until the page has said how its canvas is painted, so a page
        // never opens on a blank white one; DevTools keep it.
        let blank = self.tone.is_none() || inspector;
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
                // Behind the page, what its own canvas is painted: that's what shows beside it
                // for the frame or two its picture lags a pane's edge by.
                (false, false) => (self.tone.as_ref()).map_or_else(white, super::tone::Tone::canvas),
            })
            .child(PageSurface {
                view: cx.entity(),
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
        input::edits(page, cx)
    }
}
