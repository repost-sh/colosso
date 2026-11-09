use super::*;

#[test]
fn slots_sit_two_by_two_and_never_move() {
    assert_eq!(slot_origin(0), Point { x: 0., y: 0. });
    assert_eq!(slot_origin(1), Point { x: 1680., y: 0. });
    assert_eq!(slot_origin(2), Point { x: 0., y: 1050. });
    assert_eq!(slot_origin(3), Point { x: 1680., y: 1050. });
}

#[test]
fn the_screen_grows_right_then_down() {
    let mut layout = Layout::default();
    assert_eq!(layout.screen(), AREA);
    let mut sizes = Vec::new();
    while let Some(slot) = layout.free() {
        assert!(layout.take(slot));
        sizes.push(layout.screen());
    }
    assert_eq!(
        sizes,
        [
            Size {
                width: 1680,
                height: 1050
            },
            Size {
                width: 3360,
                height: 1050
            },
            Size {
                width: 3360,
                height: 2100
            },
            Size {
                width: 3360,
                height: 2100
            },
        ]
    );
    assert_eq!(layout.len(), 4);
    assert_eq!(layout.free(), None);
}

#[test]
fn the_screen_shrinks_only_when_the_furthest_slot_frees() {
    let mut layout = Layout::of([0, 1, 2]);
    assert!(layout.free_slot(1));
    // Slot 2 is still in the second row, below slot 0: one column, two rows.
    assert_eq!(
        layout.screen(),
        Size {
            width: 1680,
            height: 2100
        }
    );
    assert!(layout.free_slot(2));
    assert_eq!(layout.screen(), AREA);
    assert!(!layout.free_slot(2));
    assert_eq!(layout.free(), Some(1));
}

#[test]
fn a_freed_slot_is_given_again_before_a_new_one() {
    let mut layout = Layout::of([0, 1, 2]);
    layout.free_slot(0);
    assert_eq!(layout.free(), Some(0));
}

#[test]
fn every_screen_fits_screen_sharing() {
    for used in 0u8..16 {
        let layout = Layout::of((0..SLOTS).filter(|slot| used & (1 << slot) != 0));
        let screen = layout.screen();
        assert!(screen.width <= SCREEN_MAX.width && screen.height <= SCREEN_MAX.height);
    }
}

#[test]
fn slots_out_of_range_are_never_taken() {
    let mut layout = Layout::default();
    assert!(!layout.take(SLOTS));
    assert!(layout.is_empty());
    assert!(layout.take(3));
    assert!(!layout.take(3));
}
