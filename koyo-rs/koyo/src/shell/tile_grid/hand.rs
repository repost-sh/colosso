//! What's in hand over a grid of tiles: a tile of its own, taken up and let go over another slot,
//! and a page from elsewhere over the region that takes it, which lands where the tiles made room
//! for it; and where a grid's cells are.

use std::cell::{Cell, RefCell};

use gpui_kit::{Bounds, Pixels, Point, Size, point, px};

/// How far from the grid the pointer takes a tile before it goes back to its slot.
const PULL: f32 = 40.;

/// A page from elsewhere, as the grid last drew it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Newcomer {
    /// None is over the region that takes it.
    #[default]
    Away,
    /// One is, and let go it goes to the slot `at`.
    Over(usize),
    /// One was let go there: it's one of the tiles from now on, in the room made for it.
    Landed,
}

/// The tile in hand, shared by the tiles, which take it up, and their grid, which draws it
/// under the pointer, notes the slot it would go to and lets it go there; and a page from
/// elsewhere over the grid, which the region that takes it lets go.
#[derive(Default)]
pub struct TileHand {
    drag: RefCell<Option<Drag>>,
    /// Whether the tiles still slide into place after one was let go.
    pub(super) settling: Cell<bool>,
    pub(super) newcomer: Cell<Newcomer>,
    /// Where a page from elsewhere is let go to land among the tiles, as last drawn: the region
    /// round the grid that takes it.
    region: Cell<Option<Bounds<Pixels>>>,
    /// How big a tile was last drawn: the room a page from elsewhere is given.
    pub(super) tile: Cell<Option<Size<Pixels>>>,
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
pub(super) struct Held {
    pub at: usize,
    /// Where it's drawn, from the grid's corner: under the pointer, or in its own slot once
    /// it's taken away from the grid.
    pub origin: Point<Pixels>,
    pub to: usize,
    pub away: bool,
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

    /// Notes where a page from elsewhere is let go to land among the tiles, as it's drawn there:
    /// the region round the grid that takes it, drawn before the grid.
    pub fn takes_at(&self, region: Bounds<Pixels>) {
        self.region.set(Some(region));
    }

    /// A page from elsewhere let go over the region that takes it: the slot it lands in, where
    /// the tiles made room for it, if they did. From then it's one of the tiles, and the room
    /// made for it closes at once.
    pub fn lands(&self) -> Option<usize> {
        let Newcomer::Over(at) = self.newcomer.get() else {
            return None;
        };
        self.newcomer.set(Newcomer::Landed);
        Some(at)
    }

    /// Lets go of the tile in hand: the slot it leaves and the one it goes to, if it moves.
    pub(super) fn release(&self) -> Option<(usize, usize)> {
        let drag = self.drag.borrow_mut().take()?;
        self.settling.set(true);
        drag.slots.filter(|(from, to)| from != to)
    }

    /// The tile in hand among the tiles `keys` in the cells of `grid`, at `bounds`, where the
    /// pointer at `pointer` has it, noting the slot it would go to: the one its middle is
    /// nearest, the room a short last row leaves counting as the last slot. Once gpui's drag is
    /// over, or its tile is gone, there is none, and the grid settles.
    pub(super) fn place(
        &self,
        keys: &[u64],
        grid: Option<Cells>,
        bounds: Bounds<Pixels>,
        pointer: Point<Pixels>,
        held: bool,
    ) -> Option<Held> {
        let mut hand = self.drag.borrow_mut();
        let drag = hand.as_mut()?;
        let index = keys.iter().position(|key| *key == drag.key);
        let (Some(at), Some(grid), true) = (index, grid, held) else {
            self.settling.set(true);
            *hand = None;
            return None;
        };
        let slot = grid.at(at);
        let away = outside(bounds, pointer) > px(PULL);
        let origin = if away {
            slot.origin - bounds.origin
        } else {
            let free = pointer - drag.grab - bounds.origin;
            let across = (bounds.size.width - slot.size.width).max(px(0.));
            let down = (bounds.size.height - slot.size.height).max(px(0.));
            point(free.x.clamp(px(0.), across), free.y.clamp(px(0.), down))
        };
        let middle = bounds.origin + origin + point(slot.size.width / 2., slot.size.height / 2.);
        let (count, per_row) = (keys.len(), grid.per_row);
        let nearest = grid.nearest(count.div_ceil(per_row) * per_row, middle);
        let to = if away { at } else { nearest.min(count - 1) };
        (drag.slots, drag.away) = (Some((at, to)), away);
        drag.left |= away;
        Some(Held {
            at,
            origin,
            to,
            away,
        })
    }

    /// Notes where a page from elsewhere goes among the `count` tiles of `grid`, with the
    /// pointer at `pointer`, while one the grid `takes` is in hand: the cell the pointer is
    /// nearest, of theirs and the one after the last, while it's over the region that takes it.
    pub(super) fn land(
        &self,
        grid: Option<Cells>,
        count: usize,
        pointer: Point<Pixels>,
        takes: bool,
    ) -> Option<usize> {
        let over = takes && (self.region.get()).is_some_and(|region| region.contains(&pointer));
        let landing = grid.filter(|_| over).map(|grid| grid.nearest(count + 1, pointer));
        self.newcomer.set(landing.map_or(Newcomer::Away, Newcomer::Over));
        landing
    }
}

/// Where a grid's cells are, its tiles all one size: the first one's, how many to a row, and
/// the gap between them.
#[derive(Clone, Copy)]
pub(super) struct Cells {
    pub first: Bounds<Pixels>,
    pub per_row: usize,
    pub gap: Pixels,
}

impl Cells {
    /// The cell at `at`, counted along each row and on down the rows.
    pub(super) fn at(self, at: usize) -> Bounds<Pixels> {
        let per_row = self.per_row;
        let tile = self.first.size;
        let step = point(tile.width + self.gap, tile.height + self.gap);
        let (along, down) = ((at % per_row) as f32, (at / per_row) as f32);
        let origin = self.first.origin + point(step.x * along, step.y * down);
        Bounds { origin, size: tile }
    }

    /// Which of the first `count` cells has its middle nearest `spot`, the first of any as
    /// near.
    fn nearest(self, count: usize, spot: Point<Pixels>) -> usize {
        let far = |at: &usize| {
            let apart = self.at(*at).center() - spot;
            apart.x.as_f32().hypot(apart.y.as_f32())
        };
        (0..count)
            .min_by(|a, b| far(a).total_cmp(&far(b)))
            .unwrap_or_default()
    }
}

/// How far `at` is outside `bounds`, the further of across and down: none while it's over them.
fn outside(bounds: Bounds<Pixels>, at: Point<Pixels>) -> Pixels {
    let across = (bounds.left() - at.x).max(at.x - bounds.right());
    let down = (bounds.top() - at.y).max(at.y - bounds.bottom());
    across.max(down).max(px(0.))
}
