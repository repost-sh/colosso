//! Tabs dragged into a new order, along the strip or down the sidebar: the tab in hand
//! follows the pointer, the others slide out of its way
//! into the slot it left once its middle has passed theirs, and let go it slides into the
//! slot it's over. Held over the middle of another tab it joins that one instead, side by
//! side, the half the pointer is over lit for the side it takes. Pulled off the list it
//! slides back into its slot and travels as a chip, to drop on a side of a pane.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::{
    AlignItems, Along as _, AnyElement, App, Axis, BorderStyle, Bounds, Corners, DispatchPhase,
    Display, Element, ElementId, FlexDirection, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Length, MouseUpEvent, Pixels, Point, Style, Window, fill, outline,
    point, px, relative, size,
};

use hand::Holds;
use hand::List;
pub use hand::{Drop, TabHand};

mod hand;

/// How long a tab takes to slide into place.
pub const MOTION: Duration = Duration::from_millis(240);
/// The corners of a tab, and of the light over one being joined.
const RADIUS: f32 = 6.;

/// The point `along` a list running along `axis` from its start, level with it.
fn at_along(axis: Axis, along: Pixels) -> Point<Pixels> {
    Point::default().apply_along(axis, |_| along)
}

/// How far `at` is past `bounds` along `axis`, before its start or after its end: none
/// while it's within them.
fn past(axis: Axis, bounds: Bounds<Pixels>, at: Point<Pixels>) -> Pixels {
    let (at, from) = (at.along(axis), bounds.origin.along(axis));
    let to = from + bounds.size.along(axis);
    (from - at).max(at - to).max(px(0.))
}

/// How far `at` is outside `bounds` across a list along `axis`: none while it's level.
fn outside(axis: Axis, bounds: Bounds<Pixels>, at: Point<Pixels>) -> Pixels {
    past(axis.invert(), bounds, at)
}

/// Whether `at` is past either end of a list down the sidebar, where the next checkout's list
/// is, which a tab can be taken to (`docs/arc-sidebar.md` 5): the list lets it go at once, so
/// it doesn't keep a tab another list takes. Along the strip a tab stops at its ends.
fn beyond_ends(axis: Axis, bounds: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    axis == Axis::Vertical && past(axis, bounds, at) > px(0.)
}

/// What's done with a tab let go over its list.
pub type OnDrop = Rc<dyn Fn(&Drop, &mut Window, &mut App)>;

/// Tabs in a list, each drawn where its motion has it; or a project's worktrees, by the same
/// rules but for joining and leaving the list.
pub struct Reorder {
    list: u64,
    holds: Holds,
    axis: Axis,
    tabs: Vec<(u64, AnyElement)>,
    hand: Rc<TabHand>,
    gap: Pixels,
    on_drop: OnDrop,
}

/// Where the tabs are drawn this frame: how far each is from where it's laid out, which
/// is in hand, drawn over the others, and the tab it would join, with the side it takes.
pub struct Placement {
    offsets: Vec<Pixels>,
    held: Option<usize>,
    join: Option<(Bounds<Pixels>, bool)>,
}

impl Placement {
    /// The tabs in the order they're drawn: the one in hand last.
    fn order(&self) -> impl Iterator<Item = usize> {
        (0..self.offsets.len())
            .filter(|at| Some(*at) != self.held)
            .chain(self.held)
    }
}

impl Reorder {
    /// The tabs of `list`, each by a key it keeps as it moves, running along `axis` with
    /// `gap` between them, `on_drop` doing what's done with one let go over the list.
    pub fn new(
        (list, axis): (u64, Axis),
        tabs: Vec<(u64, AnyElement)>,
        hand: Rc<TabHand>,
        gap: Pixels,
        accent: Hsla,
        on_drop: OnDrop,
    ) -> Self {
        Reorder {
            list,
            holds: Holds::Tabs { accent },
            axis,
            tabs,
            hand,
            gap,
            on_drop,
        }
    }

    /// The loose pages of list `list` down the Arc sidebar, each by a key it keeps as it moves,
    /// with `gap` between them, `on_drop` doing what's done with one let go over the list: they
    /// move as tabs do and leave the list as tabs do, but join none.
    pub fn pages(
        list: u64,
        pages: Vec<(u64, AnyElement)>,
        hand: Rc<TabHand>,
        gap: Pixels,
        on_drop: OnDrop,
    ) -> Self {
        Reorder {
            list,
            holds: Holds::Pages,
            axis: Axis::Vertical,
            tabs: pages,
            hand,
            gap,
            on_drop,
        }
    }

    /// The worktrees of project `list` down the Arc sidebar, each by a key it keeps as it moves,
    /// with `gap` between them, `on_drop` doing what's done with one let go: they move as tabs
    /// do, but join none, and stay on the list wherever they're taken.
    pub fn worktrees(
        list: u64,
        worktrees: Vec<(u64, AnyElement)>,
        hand: Rc<TabHand>,
        gap: Pixels,
        on_drop: OnDrop,
    ) -> Self {
        Reorder {
            list,
            holds: Holds::Worktrees,
            axis: Axis::Vertical,
            tabs: worktrees,
            hand,
            gap,
            on_drop,
        }
    }
}

impl IntoElement for Reorder {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Reorder {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Placement;

    fn id(&self) -> Option<ElementId> {
        let name = match self.holds {
            Holds::Tabs { .. } => "tab-list",
            Holds::Worktrees => "worktree-list",
            Holds::Pages => "page-list",
        };
        Some((name, self.list).into())
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
        let gap = self.gap.into();
        let style = match self.axis {
            Axis::Horizontal => Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: size(gap, px(0.).into()),
                // Narrowing its tabs before the strip overflows.
                flex_shrink: 1.,
                min_size: size(px(0.).into(), Length::Auto),
                size: size(Length::Auto, relative(1.).into()),
                ..Style::default()
            },
            Axis::Vertical => Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                gap: size(px(0.).into(), gap),
                size: size(relative(1.).into(), Length::Auto),
                ..Style::default()
            },
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
        let (axis, gap) = (self.axis, self.gap);
        let slots: Vec<_> = tabs.iter().map(|id| window.layout_bounds(*id)).collect();
        let list = List {
            id: self.list,
            holds: self.holds,
            axis,
            keys: self.tabs.iter().map(|(key, _)| *key).collect(),
            bounds,
        };
        let pointer = window.mouse_position();
        let held = (self.hand).place(&list, &slots, pointer, cx.has_active_drag());
        // Each tab's place is a transitioned value only while one's in hand or settling:
        // the rest of the time there's no state, so a tab is where it's laid out at once.
        let moving = held.is_some() || self.hand.settling(self.list);
        let slide = Transition::new(MOTION);
        let at_once = Transition::new(Duration::ZERO);
        let mut settled = held.is_none();
        let start = bounds.origin.along(axis);
        let offsets: Vec<Pixels> = if moving {
            (slots.iter().enumerate())
                .map(|(at, slot)| {
                    let laid = slot.origin.along(axis) - start;
                    let (target, policy) = match held {
                        // Under the pointer, or back to its slot once it's off the list.
                        Some(h) if h.at == at => (h.pos, if h.off { &slide } else { &at_once }),
                        // Out of the way of the tab in hand, into the slot it left, once it's
                        // passed this one.
                        Some(h) if h.at < at && at <= h.to => (laid - (h.len + gap), &slide),
                        Some(h) if h.to <= at && at < h.at => (laid + h.len + gap, &slide),
                        _ => (laid, &slide),
                    };
                    let id = (("tab-order", self.tabs[at].0), "offset");
                    let value = transition_with_status(id, target, policy.clone(), window, cx);
                    settled &= value.status != MotionStatus::Running;
                    value.value - laid
                })
                .collect()
        } else {
            vec![px(0.); slots.len()]
        };
        if settled {
            self.hand.settled(self.list);
        }
        let join = held.and_then(|held| held.join).map(|join| {
            let slot = slots[join.onto];
            let origin = slot.origin + at_along(axis, offsets[join.onto]);
            (
                Bounds {
                    origin,
                    size: slot.size,
                },
                join.after,
            )
        });
        let placement = Placement {
            offsets,
            held: held.map(|held| held.at),
            join,
        };
        // The tab in hand last, so the pointer is over it rather than the tabs it passes.
        for at in placement.order() {
            window.with_element_offset(at_along(axis, placement.offsets[at]), |window| {
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
        if let (Some((bounds, after)), Holds::Tabs { accent }) = (placement.join, self.holds) {
            paint_join(bounds, after, accent, window);
        }
        // The list lets its own tab go, wherever over it the pointer is, before anything
        // under the pointer hears the release: off the list, a pane's side takes it.
        if placement.held.is_some() {
            let (hand, list, on_drop) = (self.hand.clone(), self.list, self.on_drop.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && let Some(drop) = hand.release(list)
                {
                    on_drop(&drop, window, cx);
                }
            });
        }
    }
}

/// Over the tab being joined, the tab in hand included so it shows through: an outline,
/// and the half the tab in hand takes washed.
fn paint_join(bounds: Bounds<Pixels>, after: bool, accent: Hsla, window: &mut Window) {
    let r = px(RADIUS);
    let (lead, trail, origin) = if after {
        (px(0.), r, point(bounds.center().x, bounds.origin.y))
    } else {
        (r, px(0.), bounds.origin)
    };
    let corners = Corners {
        top_left: lead,
        top_right: trail,
        bottom_right: trail,
        bottom_left: lead,
    };
    let half = size(bounds.size.width / 2., bounds.size.height);
    let wash = fill(Bounds { origin, size: half }, accent.opacity(0.18));
    window.paint_quad(wash.corner_radii(corners));
    window.paint_quad(outline(bounds, accent, BorderStyle::Solid).corner_radii(r));
}
