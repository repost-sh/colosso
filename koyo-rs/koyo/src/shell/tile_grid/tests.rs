//! The tiles dragged about their grid, and a page from elsewhere landing among them.

use gpui_kit::point;

use super::hand::{Cells, Newcomer};
use super::*;

/// `count` tiles 60 by 44, three to a row with 8 between them, keyed from 100, in a grid
/// whose corner is at 10, 20.
fn grid(count: usize) -> (Vec<u64>, Option<Cells>, Bounds<Pixels>) {
    let corner = point(px(10.), px(20.));
    let first = Bounds {
        origin: corner,
        size: size(px(60.), px(44.)),
    };
    let cells = Cells {
        first,
        per_row: 3,
        gap: px(8.),
    };
    let rows = count.div_ceil(3) as f32;
    let bounds = Bounds {
        origin: corner,
        size: size(px(196.), px(rows * 52. - 8.)),
    };
    ((100..).take(count).collect(), Some(cells), bounds)
}

/// The pointer holding a tile by its middle, that middle at `x`, `y` from the grid's corner.
fn middle_at(x: f32, y: f32) -> Point<Pixels> {
    point(px(10. + x), px(20. + y))
}

#[test]
fn a_tile_goes_to_the_slot_its_middle_is_nearest_across_rows_too() {
    let hand = TileHand::default();
    let (keys, cells, bounds) = grid(5);
    hand.take(100, point(px(30.), px(22.)));
    let place = |x, y| {
        let held = hand.place(&keys, cells, bounds, middle_at(x, y), true);
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
    let held = place(98., 74.);
    assert_eq!(held.to, 4);
    assert_eq!(held.origin, point(px(68.), px(52.)));
    assert_eq!(place(166., 74.).to, 4);
    assert_eq!(hand.release(), Some((0, 4)));
    assert!(hand.holding().is_none());
    assert!(hand.settling.get());
}

#[test]
fn the_tiles_between_step_into_the_slot_left_and_it_stops_at_the_grids_edges() {
    let stepped: Vec<usize> = (0..5).map(|at| shifted(at, 0, 3)).collect();
    assert_eq!(stepped, [0, 0, 1, 2, 4]);
    let stepped: Vec<usize> = (0..5).map(|at| shifted(at, 4, 1)).collect();
    assert_eq!(stepped, [0, 2, 3, 4, 4]);

    let hand = TileHand::default();
    let (keys, cells, bounds) = grid(5);
    hand.take(104, point(px(30.), px(22.)));
    let held = hand.place(&keys, cells, bounds, middle_at(-20., -10.), true);
    let held = held.expect("a tile in hand");
    assert_eq!(
        (held.to, held.origin, held.away),
        (0, Point::default(), false)
    );
}

#[test]
fn a_tile_taken_away_from_the_grid_goes_back_and_letting_go_changes_nothing() {
    let hand = TileHand::default();
    let (keys, cells, bounds) = grid(5);
    hand.take(101, point(px(30.), px(22.)));
    let held = hand.place(&keys, cells, bounds, middle_at(98., 300.), true);
    let held = held.expect("a tile in hand");
    assert!(held.away && hand.away());
    assert_eq!((held.to, held.origin), (1, point(px(68.), px(0.))));
    assert_eq!(hand.release(), None, "it's where it was");

    hand.take(101, point(px(30.), px(22.)));
    assert!(
        hand.place(&keys, cells, bounds, middle_at(98., 22.), false)
            .is_none()
    );
    assert!(hand.holding().is_none());
    hand.take(999, Point::default());
    assert!(
        hand.place(&keys, cells, bounds, middle_at(98., 22.), true)
            .is_none()
    );
    assert!(hand.holding().is_none());
}

#[test]
fn a_page_from_elsewhere_lands_in_the_cell_its_pointer_is_nearest_while_over_the_region() {
    let hand = TileHand::default();
    let (keys, cells, bounds) = grid(5);
    let land = |x, y, takes| hand.land(cells, keys.len(), middle_at(x, y), takes);
    assert_eq!(land(30., 22., true), None, "no region takes it yet");
    hand.takes_at(bounds);
    assert_eq!(land(2., 22., true), Some(0), "before the first");
    assert_eq!(land(100., 22., true), Some(1), "over the second");
    assert_eq!(land(190., 22., true), Some(2), "the end of the first row");
    assert_eq!(land(190., 80., true), Some(5), "the empty cell after the last");
    assert_eq!(land(190., 80., false), None, "nothing it takes in hand");
    assert_eq!(hand.lands(), None);

    land(100., 22., true);
    assert_eq!(hand.lands(), Some(1), "let go, it lands where it was over");
    assert_eq!(hand.newcomer.get(), Newcomer::Landed, "the room closes at once");
    assert_eq!(hand.lands(), None, "and only once");
    assert_eq!(land(500., 22., true), None, "off the region");
    assert_eq!(hand.newcomer.get(), Newcomer::Away);
}
