//! The tiles of a space as a grid they're dragged about in (`docs/arc-sidebar.md` 3): the tile
//! in hand follows the pointer, the others step out of its way into the slot it left, along
//! their row and round onto the next, and let go it slides into the slot it's over. Taken well
//! away from the grid it goes back to its own slot, and letting go there changes nothing. The
//! grid's counterpart of the tab lists' `Reorder`, which runs one way only. The projects' tiles
//! are one grid, the pinned pages' another (`docs/arc-sidebar.md` 11), each with its own hand.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::{
    AnyElement, App, Bounds, DispatchPhase, Display, Element, ElementId, GlobalElementId,
    GridTemplate, GridTemplateMinSize, InspectorElementId, IntoElement, LayoutId, Length,
    MouseUpEvent, Pixels, Point, Style, Window, point, px, relative, size,
};

/// How long a tile takes to slide into place, as a tab does.
const MOTION: Duration = Duration::from_millis(240);
/// How far from the grid the pointer takes a tile before it goes back to its slot.
const PULL: f32 = 40.;

/// The tile in hand, shared by the tiles, which take it up, and their grid, which draws it
/// under the pointer, notes the slot it would go to and lets it go there.
#[derive(Default)]
pub struct TileHand {
    drag: RefCell<Option<Drag>>,
    /// Whether the tiles still slide into place after one was let go.
    settling: Cell<bool>,
}

struct Drag {
    key: u64,
    /// Where in the tile the pointer took hold of it.
    grab: Point<Pixels>,
    /// Its slot and the slot it's over, where it goes when it's let go, as last drawn.
    slots: Option<(usize, usize)>,
    /// Whether it was well away from the grid then, back in its own slot.
    away: bool,
    /// Whether it has been well away from the grid since it was taken up.
    left: bool,
}

/// The tile in hand as the grid draws it this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    at: usize,
    /// Where it's drawn, from the grid's corner: under the pointer, or in its own slot once
    /// it's taken away from the grid.
    origin: Point<Pixels>,
    to: usize,
    away: bool,
}

impl TileHand {
    /// Takes up the tile `key`, held `grab` in from its corner.
    pub fn take(&self, key: u64, grab: Point<Pixels>) {
        *self.drag.borrow_mut() = Some(Drag {
            key,
            grab,
            slots: None,
            away: false,
            left: false,
        });
    }

    /// Whether the tile in hand has been well away from its grid since it was taken up: what
    /// takes it elsewhere shows for it from then until it's let go.
    pub fn left_grid(&self) -> bool {
        self.drag.borrow().as_ref().is_some_and(|drag| drag.left)
    }

    /// Whether the tile in hand is well away from its grid, where it's gone back to its slot:
    /// what's let go there is no move among the tiles.
    pub fn away(&self) -> bool {
        self.drag.borrow().as_ref().is_some_and(|drag| drag.away)
    }

    /// The tile in hand, by its key.
    pub fn holding(&self) -> Option<u64> {
        self.drag.borrow().as_ref().map(|drag| drag.key)
    }

    /// Lets go of the tile in hand: the slot it leaves and the one it goes to, if it moves.
    fn release(&self) -> Option<(usize, usize)> {
        let drag = self.drag.borrow_mut().take()?;
        self.settling.set(true);
        drag.slots.filter(|(from, to)| from != to)
    }

    /// The tile in hand among `slots`, those of the tiles `keys` in a grid at `bounds` with
    /// `per_row` to a row, where the pointer at `pointer` has it, noting the slot it would go
    /// to: the one its middle is nearest, the room a short last row leaves counting as the last
    /// slot. Once gpui's drag is over, or its tile is gone, there is none, and the grid settles.
    fn place(
        &self,
        keys: &[u64],
        (slots, per_row): (&[Bounds<Pixels>], usize),
        bounds: Bounds<Pixels>,
        pointer: Point<Pixels>,
        held: bool,
    ) -> Option<Held> {
        let mut hand = self.drag.borrow_mut();
        let drag = hand.as_mut()?;
        let index = keys.iter().position(|key| *key == drag.key);
        let (Some(at), true) = (index, held) else {
            self.settling.set(true);
            *hand = None;
            return None;
        };
        let slot = slots[at];
        let away = outside(bounds, pointer) > px(PULL);
        let origin = if away {
            slot.origin - bounds.origin
        } else {
            // Under the pointer, as far as the grid goes.
            let free = pointer - drag.grab - bounds.origin;
            let across = (bounds.size.width - slot.size.width).max(px(0.));
            let down = (bounds.size.height - slot.size.height).max(px(0.));
            point(free.x.clamp(px(0.), across), free.y.clamp(px(0.), down))
        };
        let middle = bounds.origin + origin + point(slot.size.width / 2., slot.size.height / 2.);
        let far = |cell: Point<Pixels>| {
            let apart = cell - middle;
            apart.x.as_f32().hypot(apart.y.as_f32())
        };
        let nearest = (cells(slots, per_row).into_iter())
            .min_by(|(_, a), (_, b)| far(*a).total_cmp(&far(*b)))
            .map_or(at, |(nearest, _)| nearest);
        let to = if away { at } else { nearest };
        (drag.slots, drag.away) = (Some((at, to)), away);
        drag.left |= away;
        Some(Held {
            at,
            origin,
            to,
            away,
        })
    }
}

/// The middle of each cell of a grid with `per_row` to a row, with the slot a tile over it goes
/// to: each tile's own, and for the cells a short last row leaves empty, the last tile's, as a
/// tile let go past the last one goes last.
fn cells(slots: &[Bounds<Pixels>], per_row: usize) -> Vec<(usize, Point<Pixels>)> {
    let mut cells: Vec<_> = (slots.iter().enumerate())
        .map(|(at, slot)| (at, slot.center()))
        .collect();
    let (count, per_row) = (slots.len(), per_row.max(1));
    // Only a grid of more than one row has a short row, and so a tile under another to tell
    // how far apart the rows are.
    if let (Some(first), Some(beside), Some(under)) =
        (slots.first(), slots.get(1), slots.get(per_row))
    {
        let across = beside.center().x - first.center().x;
        let down = under.center().y - first.center().y;
        for empty in count..count.div_ceil(per_row) * per_row {
            let cell = point(
                across * (empty % per_row) as f32,
                down * (empty / per_row) as f32,
            );
            cells.push((count - 1, first.center() + cell));
        }
    }
    cells
}

/// How far `at` is outside `bounds`, the further of across and down: none while it's over them.
fn outside(bounds: Bounds<Pixels>, at: Point<Pixels>) -> Pixels {
    let across = (bounds.left() - at.x).max(at.x - bounds.right());
    let down = (bounds.top() - at.y).max(at.y - bounds.bottom());
    across.max(down).max(px(0.))
}

/// The slot the tile laid out at `at` shows in while the one from slot `from` is held over
/// `to`: those between the two step into the slot the tile in hand left, a slot each.
fn shifted(at: usize, from: usize, to: usize) -> usize {
    if from < at && at <= to {
        at - 1
    } else if to <= at && at < from {
        at + 1
    } else {
        at
    }
}

/// What's done with a tile let go over another slot: the slot it leaves and the one it goes to.
pub type OnDrop = Rc<dyn Fn(&(usize, usize), &mut Window, &mut App)>;

/// Tiles in a grid, each drawn where its motion has it.
pub struct TileGrid {
    /// Which grid it is, of the sidebar's.
    name: &'static str,
    tiles: Vec<(u64, AnyElement)>,
    per_row: usize,
    gap: Pixels,
    hand: Rc<TileHand>,
    on_drop: OnDrop,
}

/// Where the tiles are drawn this frame: how far each is from where it's laid out, and which is
/// in hand, drawn over the others.
pub struct Placement {
    offsets: Vec<Point<Pixels>>,
    held: Option<usize>,
}

impl Placement {
    /// The tiles in the order they're drawn: the one in hand last.
    fn order(&self) -> impl Iterator<Item = usize> {
        (0..self.offsets.len())
            .filter(|at| Some(*at) != self.held)
            .chain(self.held)
    }
}

impl TileGrid {
    /// The grid called `name`: its `tiles`, each by a key it keeps as it moves, `per_row` to a
    /// row with `gap` between them, `on_drop` doing what's done with one let go over another slot.
    pub fn new(
        name: &'static str,
        tiles: Vec<(u64, AnyElement)>,
        (per_row, gap): (usize, Pixels),
        hand: Rc<TileHand>,
        on_drop: OnDrop,
    ) -> Self {
        TileGrid {
            name,
            tiles,
            per_row,
            gap,
            hand,
            on_drop,
        }
    }
}

impl IntoElement for TileGrid {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TileGrid {
    type RequestLayoutState = Vec<LayoutId>;
    type PrepaintState = Placement;

    fn id(&self) -> Option<ElementId> {
        Some(self.name.into())
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
        let tiles: Vec<LayoutId> = (self.tiles.iter_mut())
            .map(|(_, tile)| tile.request_layout(window, cx))
            .collect();
        let gap = self.gap.into();
        // Columns sharing the width evenly, so a short last row's tiles are as wide as the rest.
        let style = Style {
            display: Display::Grid,
            grid_cols: Some(GridTemplate {
                repeat: u16::try_from(self.per_row).unwrap_or(1),
                min_size: GridTemplateMinSize::Zero,
            }),
            gap: size(gap, gap),
            size: size(relative(1.).into(), Length::Auto),
            flex_shrink: 0.,
            ..Style::default()
        };
        let id = window.request_layout(style, tiles.iter().copied(), cx);
        (id, tiles)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        tiles: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let slots: Vec<_> = tiles.iter().map(|id| window.layout_bounds(*id)).collect();
        let keys: Vec<u64> = self.tiles.iter().map(|(key, _)| *key).collect();
        let pointer = window.mouse_position();
        let grid = (slots.as_slice(), self.per_row);
        let held = (self.hand).place(&keys, grid, bounds, pointer, cx.has_active_drag());
        // Each tile's place is a transitioned value only while one's in hand or settling: the
        // rest of the time there's no state, so a tile is where it's laid out at once.
        let moving = held.is_some() || self.hand.settling.get();
        let slide = Transition::new(MOTION);
        let at_once = Transition::new(Duration::ZERO);
        let mut settled = held.is_none();
        let offsets: Vec<Point<Pixels>> = if moving {
            (slots.iter().enumerate())
                .map(|(at, slot)| {
                    let laid = slot.origin - bounds.origin;
                    let (target, policy) = match held {
                        // Under the pointer, or back to its slot once it's away from the grid.
                        Some(h) if h.at == at => (h.origin, if h.away { &slide } else { &at_once }),
                        // Out of the way of the tile in hand, towards the slot it left.
                        Some(h) => {
                            let slot = slots[shifted(at, h.at, h.to)];
                            (slot.origin - bounds.origin, &slide)
                        }
                        None => (laid, &slide),
                    };
                    let id = (("tile-order", keys[at]), "place");
                    let value = transition_with_status(id, target, policy.clone(), window, cx);
                    settled &= value.status != MotionStatus::Running;
                    value.value - laid
                })
                .collect()
        } else {
            vec![Point::default(); slots.len()]
        };
        if settled {
            self.hand.settling.set(false);
        }
        let placement = Placement {
            offsets,
            held: held.map(|held| held.at),
        };
        // The tile in hand last, so the pointer is over it rather than the tiles it passes.
        for at in placement.order() {
            window.with_element_offset(placement.offsets[at], |window| {
                self.tiles[at].1.prepaint(window, cx);
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
            self.tiles[at].1.paint(window, cx);
        }
        // The grid lets its tile go wherever the pointer is, before anything under it hears
        // the release.
        if placement.held.is_some() {
            let (hand, on_drop) = (self.hand.clone(), self.on_drop.clone());
            window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && let Some(moved) = hand.release()
                {
                    on_drop(&moved, window, cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` tiles 60 by 44, three to a row with 8 between them, keyed from 100, in a grid
    /// whose corner is at 10, 20.
    fn grid(count: usize) -> (Vec<u64>, Vec<Bounds<Pixels>>, Bounds<Pixels>) {
        let corner = point(px(10.), px(20.));
        let slots: Vec<_> = (0..count)
            .map(|at| Bounds {
                origin: corner + point(px((at % 3) as f32 * 68.), px((at / 3) as f32 * 52.)),
                size: size(px(60.), px(44.)),
            })
            .collect();
        let rows = count.div_ceil(3) as f32;
        let bounds = Bounds {
            origin: corner,
            size: size(px(196.), px(rows * 52. - 8.)),
        };
        ((100..).take(count).collect(), slots, bounds)
    }

    /// The pointer holding a tile by its middle, that middle at `x`, `y` from the grid's corner.
    fn middle_at(x: f32, y: f32) -> Point<Pixels> {
        point(px(10. + x), px(20. + y))
    }

    #[test]
    fn a_tile_goes_to_the_slot_its_middle_is_nearest_across_rows_too() {
        let hand = TileHand::default();
        let (keys, slots, bounds) = grid(5);
        hand.take(100, point(px(30.), px(22.)));
        let place = |x, y| {
            let held = hand.place(&keys, (&slots, 3), bounds, middle_at(x, y), true);
            held.expect("a tile in hand")
        };
        let held = place(30., 22.);
        assert_eq!(
            (held.to, held.origin),
            (0, Point::default()),
            "where it was"
        );
        assert_eq!(place(60., 22.).to, 0, "not half way to the next yet");
        assert_eq!(place(70., 22.).to, 1);
        assert_eq!(place(166., 30.).to, 2, "the end of its row");
        // Down a row: under the second tile, which is the fifth's slot.
        let held = place(98., 74.);
        assert_eq!(held.to, 4);
        assert_eq!(held.origin, point(px(68.), px(52.)));
        // Past the last tile, in the room its row has left, it goes last, not to the slot
        // above, though that tile is the nearer.
        assert_eq!(place(166., 74.).to, 4);
        assert_eq!(hand.release(), Some((0, 4)));
        assert!(hand.holding().is_none());
        assert!(hand.settling.get());
    }

    #[test]
    fn the_tiles_between_step_into_the_slot_left_and_it_stops_at_the_grids_edges() {
        // From the first slot to the fourth, the three after it each step back one.
        let stepped: Vec<usize> = (0..5).map(|at| shifted(at, 0, 3)).collect();
        assert_eq!(stepped, [0, 0, 1, 2, 4]);
        // From the last to the second, those from the second on each step on one.
        let stepped: Vec<usize> = (0..5).map(|at| shifted(at, 4, 1)).collect();
        assert_eq!(stepped, [0, 2, 3, 4, 4]);

        let hand = TileHand::default();
        let (keys, slots, bounds) = grid(5);
        hand.take(104, point(px(30.), px(22.)));
        // A little outside, it's drawn as far as the grid goes.
        let held = hand.place(&keys, (&slots, 3), bounds, middle_at(-20., -10.), true);
        let held = held.expect("a tile in hand");
        assert_eq!(
            (held.to, held.origin, held.away),
            (0, Point::default(), false)
        );
    }

    #[test]
    fn a_tile_taken_away_from_the_grid_goes_back_and_letting_go_changes_nothing() {
        let hand = TileHand::default();
        let (keys, slots, bounds) = grid(5);
        hand.take(101, point(px(30.), px(22.)));
        let held = hand.place(&keys, (&slots, 3), bounds, middle_at(98., 300.), true);
        let held = held.expect("a tile in hand");
        assert!(held.away && hand.away());
        assert_eq!((held.to, held.origin), (1, point(px(68.), px(0.))));
        assert_eq!(hand.release(), None, "it's where it was");

        // gpui's drag over, or the tile gone, the grid has none in hand and settles.
        hand.take(101, point(px(30.), px(22.)));
        let grid = (slots.as_slice(), 3);
        assert!(
            hand.place(&keys, grid, bounds, middle_at(98., 22.), false)
                .is_none()
        );
        assert!(hand.holding().is_none());
        hand.take(999, Point::default());
        assert!(
            hand.place(&keys, grid, bounds, middle_at(98., 22.), true)
                .is_none()
        );
        assert!(hand.holding().is_none());
    }
}
