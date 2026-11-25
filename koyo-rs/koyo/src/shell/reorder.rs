//! Tabs dragged into a new order, along the strip or down the sidebar: the tab in hand
//! follows the pointer, the others slide out of its way
//! into the slot it left once its middle has passed theirs, and let go it slides into the
//! slot it's over. Held over the middle of another tab it joins that one instead, side by
//! side, the half the pointer is over lit for the side it takes. Pulled off the list it
//! slides back into its slot and travels as a chip, to drop on a side of a pane.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::{
    AlignItems, Along as _, AnyElement, App, AppContext as _, Axis, BorderStyle, Bounds, Context,
    Corners, DispatchPhase, Display, Div, Element, ElementId, Empty, FlexDirection,
    GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Length, MouseUpEvent,
    Pixels, Point, Render, Stateful, StatefulInteractiveElement as _, Style, Window, fill,
    outline, point, px, relative, size,
};

use hand::{Holds, Laid};
pub use hand::{Drop, TabHand};

use super::Shell;

/// A row or a tile in hand. Nothing's drawn for it under the pointer: its list or grid draws it
/// there.
pub struct InHand;

impl Render for InHand {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl TabHand {
    /// Has `row`, known by `key` in slot `at` of `list`, taken up by the list as it's dragged
    /// along `axis`, held where the pointer took hold of it: nothing's drawn for it under the
    /// pointer, as the list draws it there (`InHand`).
    pub fn lift(
        self: &Rc<Self>,
        row: Stateful<Div>,
        (list, key, at): (List, u64, usize),
        axis: Axis,
    ) -> Stateful<Div> {
        let hand = Rc::clone(self);
        row.on_drag(InHand, move |_, grab, _, cx| {
            hand.take(list, key, at, grab.along(axis));
            cx.new(|_| InHand)
        })
    }
}

/// What's done with a row let go over its list, rows only moving: `moved` has the shell take the
/// row from its slot to the one it's over, as `from` and `to`.
pub fn on_move(
    cx: &Context<Shell>,
    moved: impl Fn(&mut Shell, usize, usize, &mut Context<Shell>) + 'static,
) -> OnDrop {
    Rc::new(cx.listener(move |this, drop: &Drop, _, cx| {
        if let Drop::Move { from, to } = *drop {
            moved(this, from, to, cx);
        }
    }))
}

/// Which list a tab or a row is dragged along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    /// The tabs of the checkout `(project, branch)`, along the top rail or down the sidebar.
    Checkout(usize, usize),
    /// The worktrees of the project at `project`.
    Worktrees(usize),
    /// The space's projects.
    Projects,
    /// The space's loose pages, down the sidebar or along the top rail.
    Loose,
    /// The spaces' marks, along the sidebar's foot.
    Spaces,
}

impl List {
    /// What the list is known by among the window's elements.
    fn element_id(self) -> ElementId {
        match self {
            List::Checkout(project, branch) => format!("tab-list-{project}-{branch}").into(),
            List::Worktrees(project) => ("worktree-list", project).into(),
            List::Projects => "project-list".into(),
            List::Loose => "loose-list".into(),
            List::Spaces => "space-marks".into(),
        }
    }
}

/// What a project, or a worktree's place in the sidebar, is known by as it moves, whatever its
/// place: its folder's.
pub fn key_of(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

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
/// is, which a tab can be taken to (`docs/sidebar.md` 5): the list lets it go at once, so
/// it doesn't keep a tab another list takes. Along the strip a tab stops at its ends.
fn beyond_ends(axis: Axis, bounds: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    axis == Axis::Vertical && past(axis, bounds, at) > px(0.)
}

/// What's done with a tab let go over its list.
pub type OnDrop = Rc<dyn Fn(&Drop, &mut Window, &mut App)>;

/// Tabs in a list, each drawn where its motion has it; or rows, by the same rules but for joining
/// and, as `Holds` says, leaving the list.
pub struct Reorder {
    list: List,
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

/// The `count` tabs or tiles of a list or a grid in the order they're drawn: the one `held` in
/// hand last, over the others.
pub(super) fn drawing_order(count: usize, held: Option<usize>) -> impl Iterator<Item = usize> {
    (0..count).filter(move |at| Some(*at) != held).chain(held)
}

impl Reorder {
    /// The tabs of `list`, each by a key it keeps as it moves, running along `axis` with
    /// `gap` between them, `on_drop` doing what's done with one let go over the list.
    pub fn new(
        (list, axis): (List, Axis),
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

    /// The space's loose pages, down the sidebar or along the top rail as `axis` says, each by a
    /// key it keeps as it moves, with `gap` between them, `on_drop` doing what's done with one let
    /// go over the list: they move as tabs do and leave the list as tabs do, but join none, and
    /// they take the tabs of the other lists, which are off their own over them.
    pub(super) fn pages(
        axis: Axis,
        pages: Vec<(u64, AnyElement)>,
        hand: Rc<TabHand>,
        gap: Pixels,
        on_drop: OnDrop,
    ) -> Self {
        Reorder {
            list: List::Loose,
            holds: Holds::Pages,
            axis,
            tabs: pages,
            hand,
            gap,
            on_drop,
        }
    }

    /// Rows of `list`, running along `axis`, that move as tabs do but join none: down the
    /// sidebar a project's worktrees and the space's projects, which stay on the list wherever
    /// they're taken; along its foot the spaces' marks, which, as `leaves` says, leave it as tabs
    /// do, one pulled off sliding back into its slot to do nothing let go there. Each is known by
    /// a key it keeps as it moves, with `gap` between them, `on_drop` doing what's done with one
    /// let go.
    pub fn rows(
        (list, axis, leaves): (List, Axis, bool),
        rows: Vec<(u64, AnyElement)>,
        hand: Rc<TabHand>,
        gap: Pixels,
        on_drop: OnDrop,
    ) -> Self {
        Reorder {
            list,
            holds: Holds::Rows { leaves },
            axis,
            tabs: rows,
            hand,
            gap,
            on_drop,
        }
    }
}

impl Element for Reorder {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Placement;

    fn id(&self) -> Option<ElementId> {
        Some(self.list.element_id())
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
        let list = Laid {
            id: self.list,
            holds: self.holds,
            axis,
            keys: self.tabs.iter().map(|(key, _)| *key).collect(),
            bounds,
        };
        if self.holds == Holds::Pages {
            self.hand.takes_for(self.list, bounds);
        }
        let pointer = window.mouse_position();
        let held = (self.hand).place(&list, &slots, pointer, cx.has_active_drag());
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
                        Some(h) if h.at == at => (h.pos, if h.off { &slide } else { &at_once }),
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
        for at in drawing_order(placement.offsets.len(), placement.held) {
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
        for at in drawing_order(placement.offsets.len(), placement.held) {
            self.tabs[at].1.paint(window, cx);
        }
        if let (Some((bounds, after)), Holds::Tabs { accent }) = (placement.join, self.holds) {
            paint_join(bounds, after, accent, window);
        }
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
