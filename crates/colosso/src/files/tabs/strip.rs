//! How the tabs move along the strip while one is in hand: that one follows the mouse, the
//! others slide out of its way into the slot it left once it's passed their middle, and when
//! it's let go it slides into the slot it's over. Otherwise tabs are simply where they're
//! laid out: one opening or closing moves the rest at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::{
    AlignItems, AnyElement, App, Bounds, Display, Element, ElementId, EntityId, FlexDirection,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Length, Pixels, Style, Window,
    point, px, relative, size,
};

/// How long a tab takes to slide into place.
const MOTION: Duration = Duration::from_millis(240);

/// The tab in hand, shared by the panel, which takes it up and lets it go, and the strip,
/// which draws it under the mouse and notes the slot it's over.
#[derive(Default)]
pub struct Hand {
    drag: RefCell<Option<Drag>>,
    /// Whether tabs are still sliding into place after the last one was let go.
    settling: Cell<bool>,
}

struct Drag {
    key: EntityId,
    /// How far into the tab the mouse took hold of it.
    grab: Pixels,
    /// The slot it's over, where it goes when it's let go.
    to: usize,
}

/// The tab in hand, as drawn: its index, its left edge from the strip's, its width, and
/// the slot it's over.
#[derive(Clone, Copy)]
struct Dragged {
    at: usize,
    x: Pixels,
    width: Pixels,
    to: usize,
}

impl Hand {
    /// Takes up the tab `key`, in slot `at`, `grab` pixels in from its left edge.
    pub fn take(&self, key: EntityId, at: usize, grab: Pixels) {
        *self.drag.borrow_mut() = Some(Drag { key, grab, to: at });
    }

    /// The tab in hand, if one is.
    pub fn holding(&self) -> Option<EntityId> {
        self.drag.borrow().as_ref().map(|drag| drag.key)
    }

    /// Lets go of the tab in hand: which it is, and the slot it's over.
    pub fn release(&self) -> Option<(EntityId, usize)> {
        let drag = self.drag.borrow_mut().take()?;
        self.settling.set(true);
        Some((drag.key, drag.to))
    }

    /// The tab in hand among `slots`, where the mouse has it within `strip`, the slot it's
    /// over noted for when it's let go. Once gpui's drag is over, or its tab is gone, there
    /// is none, and the tabs settle.
    fn place(
        &self,
        slots: &[(EntityId, Bounds<Pixels>)],
        strip: Bounds<Pixels>,
        mouse: Pixels,
        held: bool,
    ) -> Option<Dragged> {
        let mut hand = self.drag.borrow_mut();
        let dragged = hand.as_ref().filter(|_| held).and_then(|drag| {
            let at = slots.iter().position(|(key, _)| *key == drag.key)?;
            let width = slots[at].1.size.width;
            let x = (mouse - drag.grab - strip.origin.x)
                .clamp(px(0.), (strip.size.width - width).max(px(0.)));
            // Its slot: past the middle of each tab left of its own.
            let middle = strip.origin.x + x + width / 2.;
            let to = (slots.iter().enumerate())
                .filter(|(other, (_, slot))| *other != at && slot.center().x < middle)
                .count();
            Some(Dragged { at, x, width, to })
        });
        match (hand.as_mut(), dragged) {
            (Some(drag), Some(dragged)) => drag.to = dragged.to,
            (Some(_), None) => {
                *hand = None;
                self.settling.set(true);
            }
            (None, _) => {}
        }
        dragged
    }
}

/// The tabs in a row, each drawn where its motion has it.
pub struct Strip {
    tabs: Vec<(EntityId, AnyElement)>,
    hand: Rc<Hand>,
    gap: Pixels,
}

/// Where the tabs are drawn this frame: how far each is from where it's laid out, and
/// which is in hand, drawn over the others.
pub struct Placement {
    offsets: Vec<Pixels>,
    dragged: Option<usize>,
}

impl Placement {
    /// The tabs in the order they're drawn: the one in hand last.
    fn order(&self) -> impl Iterator<Item = usize> {
        (0..self.offsets.len())
            .filter(|at| Some(*at) != self.dragged)
            .chain(self.dragged)
    }
}

impl Strip {
    pub fn new(tabs: Vec<(EntityId, AnyElement)>, hand: Rc<Hand>, gap: Pixels) -> Self {
        Strip { tabs, hand, gap }
    }
}

impl IntoElement for Strip {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Strip {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Placement;

    fn id(&self) -> Option<ElementId> {
        Some("files-tab-strip".into())
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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let tabs: Vec<LayoutId> = (self.tabs.iter_mut())
            .map(|(_, tab)| tab.request_layout(window, cx))
            .collect();
        let style = Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            align_items: Some(AlignItems::Center),
            flex_shrink: 0.,
            gap: size(self.gap.into(), px(0.).into()),
            size: size(Length::Auto, relative(1.).into()),
            ..Style::default()
        };
        let id = window.request_layout(style, tabs.iter().copied(), cx);
        (id, tabs)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        tabs: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let gap = self.gap;
        let slots: Vec<_> = (self.tabs.iter().zip(tabs.iter()))
            .map(|((key, _), id)| (*key, window.layout_bounds(*id)))
            .collect();
        let mouse = window.mouse_position().x;
        let dragged = (self.hand).place(&slots, bounds, mouse, cx.has_active_drag());
        // Each tab's place is a transitioned value only while one's in hand or settling:
        // the rest of the time there's no state, so a tab is where it's laid out at once.
        let moving = dragged.is_some() || self.hand.settling.get();
        let slide = Transition::new(MOTION);
        let at_once = Transition::new(Duration::ZERO);
        let mut settled = dragged.is_none();
        let offsets = (slots.iter().enumerate())
            .map(|(at, (key, slot))| {
                let x = slot.origin.x - bounds.origin.x;
                if !moving {
                    return px(0.);
                }
                let (target, policy) = match dragged {
                    Some(Dragged { at: held, x, .. }) if held == at => (x, &at_once),
                    // Out of the way of the tab in hand, into the slot it left, once it's
                    // passed this one.
                    Some(Dragged { at: held, width, to, .. }) if held < at && at <= to => {
                        (x - (width + gap), &slide)
                    }
                    Some(Dragged { at: held, width, to, .. }) if to <= at && at < held => {
                        (x + width + gap, &slide)
                    }
                    _ => (x, &slide),
                };
                let id = (("files-tab", key.as_u64()), "x");
                let value = transition_with_status(id, target, policy.clone(), window, cx);
                settled &= value.status != MotionStatus::Running;
                value.value - x
            })
            .collect();
        if settled {
            self.hand.settling.set(false);
        }
        let placement = Placement { offsets, dragged: dragged.map(|dragged| dragged.at) };
        // The tab in hand last, so the mouse is over it rather than the tabs it passes.
        for at in placement.order() {
            window.with_element_offset(point(placement.offsets[at], px(0.)), |window| {
                self.tabs[at].1.prepaint(window, cx);
            });
        }
        placement
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        placement: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for at in placement.order() {
            self.tabs[at].1.paint(window, cx);
        }
    }
}
