//! Tiles in a grid they're dragged about in (`docs/arc-sidebar.md` 3, 11): the tile in hand
//! follows the pointer, the others step out of its way into the slot it left, along their row
//! and round onto the next, and let go it slides into the slot it's over. Taken well away from
//! the grid it goes back to its own slot, and letting go there changes nothing. The grid's
//! counterpart of the tab lists' `Reorder`, which runs one way only. The Arc sidebar's projects
//! are one grid, the pinned pages another, down the Arc sidebar or along the Orca layout's top
//! rail, each with its own hand. A grid that takes a page from elsewhere, as the pinned pages
//! take a page's tab or a loose page, lays itself out while one is over the region that takes
//! it as it will be once it's among the tiles: every tile from the slot the pointer is nearest
//! on steps one along, the slot it leaves lit, and let go the page lands there.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::{
    AlignItems, AnyElement, App, BorderStyle, Bounds, DispatchPhase, Display, Element, ElementId,
    FlexDirection, GlobalElementId, GridTemplate, GridTemplateMinSize, Hsla, InspectorElementId,
    LayoutId, Length, MouseUpEvent, Pixels, Point, Size, Style, Window, fill, outline, px,
    relative, size,
};

/// How strong the wash over the slot a page from elsewhere lands in is.
const WASH: f32 = 0.14;

mod hand;

use super::reorder::{MOTION, drawing_order};
use hand::{Cells, Held, Newcomer};
pub use hand::TileHand;

/// How a grid lays its tiles out.
#[derive(Clone, Copy)]
pub enum Arrange {
    /// As many to a row as the function says for how many there are, sharing the grid's width,
    /// a row under another: the Arc sidebar's.
    Grid(fn(usize) -> usize),
    /// One row, each tile as wide as it is: the Orca layout's top rail.
    Row,
}

impl Arrange {
    /// How many cells a row holds with `count` of them: a row of the Orca rail holds them all,
    /// and one more.
    fn per_row(self, count: usize) -> usize {
        match self {
            Arrange::Grid(per_row) => per_row(count).max(1),
            Arrange::Row => count + 1,
        }
    }
}

/// How the slot a page from elsewhere lands in is lit: washed in `accent` and outlined in it,
/// dashed, its corners rounded as a tile's are.
#[derive(Clone, Copy)]
pub struct Landing {
    pub accent: Hsla,
    pub corners: Pixels,
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
    /// Which grid it is, of the window's.
    name: &'static str,
    tiles: Vec<(u64, AnyElement)>,
    arrange: Arrange,
    gap: Pixels,
    hand: Rc<TileHand>,
    on_drop: OnDrop,
    /// How it lights where a page from elsewhere lands, while one it takes is in hand.
    takes: Option<Landing>,
}

/// Where the tiles are drawn this frame: how far each is from where it's laid out, which is in
/// hand, drawn over the others, and the slot lit where a page from elsewhere lands.
pub struct Placement {
    offsets: Vec<Point<Pixels>>,
    held: Option<usize>,
    lit: Option<(Bounds<Pixels>, Landing)>,
}

impl TileGrid {
    /// The grid called `name`: its `tiles`, each by a key it keeps as it moves, laid out as
    /// `arrange` says with `gap` between them, `on_drop` doing what's done with one let go over
    /// another slot.
    pub fn new(
        name: &'static str,
        tiles: Vec<(u64, AnyElement)>,
        (arrange, gap): (Arrange, Pixels),
        hand: Rc<TileHand>,
        on_drop: OnDrop,
    ) -> Self {
        TileGrid {
            name,
            tiles,
            arrange,
            gap,
            hand,
            on_drop,
            takes: None,
        }
    }

    /// A page from elsewhere that it takes is in hand: over the region its hand notes
    /// (`TileHand::takes_at`), the tiles make room for it where it would land, that slot lit
    /// as `landing` says.
    pub fn taking(self, landing: Landing) -> Self {
        TileGrid {
            takes: Some(landing),
            ..self
        }
    }

    /// The room laid out after the tiles for a page from elsewhere: a tile's size, opening while
    /// one is over the region that takes it and closing once none is, at once when it's landed,
    /// as it's a tile then. None while it's closed, or before a tile has been drawn.
    fn room(&self, window: &mut Window, cx: &mut App) -> Option<Size<Length>> {
        let tile = self.hand.tile.get()?;
        let full = match self.arrange {
            Arrange::Row => tile.width,
            Arrange::Grid(_) => tile.height,
        };
        let (target, policy) = match self.hand.newcomer.get() {
            Newcomer::Over(_) => (full, Transition::new(MOTION)),
            Newcomer::Away => (px(0.), Transition::new(MOTION)),
            Newcomer::Landed => (px(0.), Transition::new(Duration::ZERO)),
        };
        let open = transition_with_status((self.name, "room"), target, policy, window, cx).value;
        (open > px(0.)).then(|| match self.arrange {
            Arrange::Row => size(open.into(), tile.height.into()),
            Arrange::Grid(_) => size(Length::Auto, open.into()),
        })
    }

    /// How the grid lays out `count` cells, the tiles and any room after them.
    fn style(&self, count: usize) -> Style {
        let gap = self.gap.into();
        match self.arrange {
            Arrange::Grid(_) => Style {
                display: Display::Grid,
                grid_cols: Some(GridTemplate {
                    repeat: u16::try_from(self.arrange.per_row(count)).unwrap_or(1),
                    min_size: GridTemplateMinSize::Zero,
                }),
                gap: size(gap, gap),
                size: size(relative(1.).into(), Length::Auto),
                flex_shrink: 0.,
                ..Style::default()
            },
            Arrange::Row => Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: size(gap, px(0.).into()),
                flex_shrink: 0.,
                ..Style::default()
            },
        }
    }

    /// How far each tile is drawn from where it's laid out among `slots`, in a grid at
    /// `bounds`: the one `held` where the pointer has it and those between it and the slot it's
    /// over in the slot next to theirs, or, where a page from elsewhere `lands`, every tile from
    /// there on in the cell after its own; each sliding there, and back once nothing's in hand.
    fn offsets(
        &self,
        (slots, bounds): (&[Bounds<Pixels>], Bounds<Pixels>),
        (held, lands): (Option<Held>, Option<(usize, Cells)>),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<Point<Pixels>> {
        if lands.is_some() {
            self.hand.settling.set(true);
        }
        if held.is_none() && !self.hand.settling.get() {
            return vec![Point::default(); slots.len()];
        }
        let slide = Transition::new(MOTION);
        let at_once = Transition::new(Duration::ZERO);
        let mut settled = held.is_none() && lands.is_none();
        let offsets = (slots.iter().enumerate())
            .map(|(at, slot)| {
                let laid = slot.origin - bounds.origin;
                let (target, policy) = match (held, lands) {
                    (Some(h), _) if h.at == at => {
                        (h.origin, if h.away { &slide } else { &at_once })
                    }
                    (Some(h), _) => {
                        let slot = slots[shifted(at, h.at, h.to)];
                        (slot.origin - bounds.origin, &slide)
                    }
                    (None, Some((to, grid))) => {
                        let cell = grid.at(at + usize::from(at >= to));
                        (cell.origin - bounds.origin, &slide)
                    }
                    (None, None) => (laid, &slide),
                };
                let id = (("tile-order", self.tiles[at].0), "place");
                let value = transition_with_status(id, target, policy.clone(), window, cx);
                settled &= value.status != MotionStatus::Running;
                value.value - laid
            })
            .collect();
        if settled {
            self.hand.settling.set(false);
        }
        offsets
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
        let room = self.room(window, cx);
        let mut cells: Vec<LayoutId> = (self.tiles.iter_mut())
            .map(|(_, tile)| tile.request_layout(window, cx))
            .collect();
        if let Some(room) = room {
            let style = Style {
                size: room,
                flex_shrink: 0.,
                ..Style::default()
            };
            cells.push(window.request_layout(style, std::iter::empty(), cx));
        }
        let id = window.request_layout(self.style(cells.len()), cells.iter().copied(), cx);
        (id, cells)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        cells: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let laid: Vec<_> = cells.iter().map(|id| window.layout_bounds(*id)).collect();
        let slots = &laid[..self.tiles.len()];
        self.hand.tile.set(slots.first().map(|slot| slot.size));
        let keys: Vec<u64> = self.tiles.iter().map(|(key, _)| *key).collect();
        let (pointer, dragging) = (window.mouse_position(), cx.has_active_drag());
        let per_row = self.arrange.per_row(laid.len());
        let grid = (slots.first()).map(|&first| Cells {
            first,
            per_row,
            gap: self.gap,
        });
        let held = (self.hand).place(&keys, grid, bounds, pointer, dragging);
        let takes = self.takes.is_some() && held.is_none() && dragging;
        let lands = (self.hand.land(grid, slots.len(), pointer, takes)).zip(grid);
        let offsets = self.offsets((slots, bounds), (held, lands), window, cx);
        let lit = (lands.zip(self.takes))
            .map(|((at, grid), landing)| (grid.at(at).intersect(&bounds), landing));
        let placement = Placement {
            offsets,
            held: held.map(|held| held.at),
            lit,
        };
        for at in drawing_order(placement.offsets.len(), placement.held) {
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
        if let Some((slot, landing)) = placement.lit {
            let corners = landing.corners;
            window.paint_quad(fill(slot, landing.accent.opacity(WASH)).corner_radii(corners));
            let edge = outline(slot, landing.accent, BorderStyle::Dashed);
            window.paint_quad(edge.corner_radii(corners));
        }
        for at in drawing_order(placement.offsets.len(), placement.held) {
            self.tiles[at].1.paint(window, cx);
        }
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
mod tests;
