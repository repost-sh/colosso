//! The tab in hand, and where among its list's tabs it would go: the pointer past the middle
//! of a tab moves it past that one, over the middle of one it joins that one, and pulled
//! off the list it's a chip for a pane's side.

use std::cell::{Cell, RefCell};

use gpui_kit::{Along as _, Axis, Bounds, Hsla, Pixels, Point, px};

use super::{beyond_ends, outside};

/// The middle share of a tab that joins what's held over it; the rest either side reorders.
const JOIN_ZONE: f32 = 0.6;
/// How far across its list a tab is pulled before it's off it. Back on it needs the pointer
/// over the list again, so a wobble at the edge doesn't flip between the two.
const PULL: f32 = 24.;

/// The tab in hand, shared by the rows, which take it up, the lists, which draw it under
/// the pointer, note where it would go and let it go there, and the chip under the
/// pointer, shown only once it's off its list.
#[derive(Default)]
pub struct TabHand {
    drag: RefCell<Option<Drag>>,
    /// The list whose tabs still slide into place after one was let go.
    settling: Cell<Option<u64>>,
}

struct Drag {
    list: u64,
    key: u64,
    /// How far into the tab the pointer took hold of it, along the list.
    grab: Pixels,
    /// Its slot, as last drawn.
    from: usize,
    /// The slot it's over, where it goes when it's let go.
    to: usize,
    join: Option<Join>,
    /// Whether it's off its list, a chip over the panes.
    off: bool,
}

/// The tab another one would join, and whether what joins it goes after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Join {
    pub onto: usize,
    pub after: bool,
}

/// What letting go of a tab over its list does, by slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// The tab at `from` goes to slot `to`, counted among the others.
    Move { from: usize, to: usize },
    /// The tab at `from` joins the tab at `onto`, on its right when `after`.
    Join {
        from: usize,
        onto: usize,
        after: bool,
    },
}

/// The tab in hand as a list draws it this frame.
#[derive(Clone, Copy)]
pub(super) struct Held {
    pub at: usize,
    /// Where it's drawn, from the list's start.
    pub pos: Pixels,
    pub len: Pixels,
    pub to: usize,
    pub join: Option<Join>,
    pub off: bool,
}

impl TabHand {
    /// Takes up the tab `key` of `list`, in slot `at`, held `grab` in from its start.
    pub fn take(&self, list: u64, key: u64, at: usize, grab: Pixels) {
        *self.drag.borrow_mut() = Some(Drag {
            list,
            key,
            grab,
            from: at,
            to: at,
            join: None,
            off: false,
        });
    }

    /// The tab in hand, by its key.
    pub fn holding(&self) -> Option<u64> {
        self.drag.borrow().as_ref().map(|drag| drag.key)
    }

    /// Whether the tab in hand would join the one it's over.
    pub fn joining(&self) -> bool {
        self.drag
            .borrow()
            .as_ref()
            .is_some_and(|drag| drag.join.is_some())
    }

    /// Whether the chip under the pointer shows: no list's tab is in hand, or it's off its
    /// list.
    pub fn chip_shows(&self) -> bool {
        self.drag.borrow().as_ref().is_none_or(|drag| drag.off)
    }

    /// Whether `list`'s tabs are still sliding into place after one was let go.
    pub(super) fn settling(&self, list: u64) -> bool {
        self.settling.get() == Some(list)
    }

    /// `list`'s tabs are all in place.
    pub(super) fn settled(&self, list: u64) {
        if self.settling(list) {
            self.settling.set(None);
        }
    }

    /// Lets go of the tab in hand: what it does, when it's `list`'s and on it.
    pub(super) fn release(&self, list: u64) -> Option<Drop> {
        let drag = self.drag.borrow_mut().take()?;
        self.settling.set(Some(drag.list));
        if drag.list != list || drag.off {
            return None;
        }
        Some(match drag.join {
            Some(Join { onto, after }) => Drop::Join {
                from: drag.from,
                onto,
                after,
            },
            None => Drop::Move {
                from: drag.from,
                to: drag.to,
            },
        })
    }

    /// The tab in hand among `slots` of `list`, where the pointer at `at` has it, noting
    /// where it would go. Once gpui's drag is over, or its tab is gone, there is none, and
    /// its list settles.
    pub(super) fn place(
        &self,
        list: &List,
        slots: &[Bounds<Pixels>],
        at: Point<Pixels>,
        held: bool,
    ) -> Option<Held> {
        let mut hand = self.drag.borrow_mut();
        let drag = hand.as_mut()?;
        if drag.list != list.id && held {
            return None;
        }
        let index = list.keys.iter().position(|key| *key == drag.key);
        let (Some(index), true) = (index, held) else {
            self.settling.set(Some(drag.list));
            *hand = None;
            return None;
        };
        let (axis, bounds) = (list.axis, list.bounds);
        let outside = outside(axis, bounds, at);
        // A worktree joins none, and has nowhere else to go: it stays on its list, wherever the
        // pointer takes it.
        let tabs = matches!(list.holds, Holds::Tabs { .. });
        drag.off = tabs
            && (beyond_ends(axis, bounds, at)
                || if drag.off {
                    outside > px(0.)
                } else {
                    outside > px(PULL)
                });
        drag.from = index;
        let start = bounds.origin.along(axis);
        let len = slots[index].size.along(axis);
        let pos = if drag.off {
            (drag.to, drag.join) = (index, None);
            slots[index].origin.along(axis) - start
        } else {
            // By the pointer, as the tab's own place stops at the list's ends: over the middle
            // of another tab it joins that one, else it's moved past the middle of each one
            // before.
            let probe = at.along(axis);
            let others = (slots.iter().enumerate()).filter(|(other, _)| *other != index);
            let over_middle = others.clone().find_map(|(onto, slot)| {
                let (from, extent) = (slot.origin.along(axis), slot.size.along(axis));
                let inset = extent * ((1. - JOIN_ZONE) / 2.);
                let over = probe >= from + inset && probe <= from + extent - inset;
                over.then(|| Join {
                    onto,
                    after: at.x > slot.center().x,
                })
            });
            drag.join = over_middle.filter(|_| tabs);
            drag.to = match drag.join {
                // The tab joined stays where it is, those between it and the slot left out of
                // the way.
                Some(Join { onto, .. }) if onto > index => onto - 1,
                Some(Join { onto, .. }) => onto + 1,
                None => (others)
                    .filter(|(_, slot)| slot.center().along(axis) < probe)
                    .count(),
            };
            (probe - drag.grab - start).clamp(px(0.), (bounds.size.along(axis) - len).max(px(0.)))
        };
        Some(Held {
            at: index,
            pos,
            len,
            to: drag.to,
            join: drag.join,
            off: drag.off,
        })
    }
}

/// What a list holds, which says what one held over it can do besides move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Holds {
    /// A checkout's tabs: one held over another's middle joins it, lit in `accent`, and one
    /// taken off the list is a chip for a pane's side or, down the sidebar, another checkout's
    /// list.
    Tabs { accent: Hsla },
    /// A project's worktrees in the Arc sidebar (`docs/arc-sidebar.md` 5), which only move.
    Worktrees,
}

/// A list's own facts beside its tabs: which list it is, what it holds, which way it runs, its
/// tabs' keys, and where it is.
pub(super) struct List {
    pub id: u64,
    pub holds: Holds,
    pub axis: Axis,
    pub keys: Vec<u64>,
    pub bounds: Bounds<Pixels>,
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, size};

    use super::*;

    /// `count` tabs 100 wide in a strip, keyed from 100.
    fn strip(count: usize) -> (List, Vec<Bounds<Pixels>>) {
        let slots = (0..count)
            .map(|at| Bounds {
                origin: point(px(at as f32 * 100.), px(0.)),
                size: size(px(100.), px(28.)),
            })
            .collect();
        let list = List {
            id: 7,
            holds: Holds::Tabs {
                accent: Hsla::default(),
            },
            axis: Axis::Horizontal,
            keys: (100..).take(count).collect(),
            bounds: Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(count as f32 * 100.), px(28.)),
            },
        };
        (list, slots)
    }

    fn at(x: f32, y: f32) -> Point<Pixels> {
        point(px(x), px(y))
    }

    #[test]
    fn a_tab_moves_past_the_edge_of_a_tab_and_joins_its_middle() {
        let hand = TabHand::default();
        let (list, slots) = strip(3);
        // Taken by its middle.
        hand.take(7, 100, 0, px(50.));
        let place = |x| hand.place(&list, &slots, at(x, 14.), true).unwrap();
        let held = place(115.);
        assert_eq!((held.to, held.join), (0, None), "not into its middle yet");
        let held = place(140.);
        assert_eq!(
            held.join,
            Some(Join {
                onto: 1,
                after: false
            }),
            "its left half"
        );
        assert_eq!(held.to, 0, "the tab joined stays where it is");
        assert!(hand.joining());
        let held = place(170.);
        assert_eq!(
            held.join,
            Some(Join {
                onto: 1,
                after: true
            }),
            "its right half"
        );
        let held = place(190.);
        assert_eq!(
            (held.to, held.join),
            (1, None),
            "into its far edge, past it"
        );
        assert!(!hand.joining());
        let held = place(240.);
        assert_eq!(
            held.join,
            Some(Join {
                onto: 2,
                after: false
            })
        );
        assert_eq!(held.to, 1, "the tab passed stays out of the way");
        assert_eq!(
            hand.release(7),
            Some(Drop::Join {
                from: 0,
                onto: 2,
                after: false
            })
        );
        assert!(hand.holding().is_none());

        hand.take(7, 102, 2, px(50.));
        // Its own place stops at the strip's start, where its middle is over the first
        // tab's middle; the pointer at that tab's edge still moves it first.
        let held = hand.place(&list, &slots, at(10., 14.), true).unwrap();
        assert_eq!((held.pos, held.to, held.join), (px(0.), 0, None));
        assert_eq!(hand.release(7), Some(Drop::Move { from: 2, to: 0 }));
    }

    #[test]
    fn a_tab_pulled_off_its_list_goes_back_and_does_nothing_there() {
        let hand = TabHand::default();
        let (list, slots) = strip(2);
        hand.take(7, 100, 0, px(50.));
        assert!(!hand.chip_shows(), "on its list the tab itself is in hand");
        hand.place(&list, &slots, at(150., 40.), true);
        assert!(
            !hand.chip_shows(),
            "a little below the strip is still on it"
        );
        let held = hand.place(&list, &slots, at(150., 80.), true).unwrap();
        assert!(held.off && held.join.is_none() && held.to == 0 && held.pos == px(0.));
        assert!(hand.chip_shows());
        hand.place(&list, &slots, at(150., 40.), true);
        assert!(hand.chip_shows(), "back near the strip it's still off");
        hand.place(&list, &slots, at(150., 14.), true);
        assert!(!hand.chip_shows(), "over the strip it's back on");
        hand.place(&list, &slots, at(150., 80.), true);
        assert_eq!(hand.release(7), None);
    }

    #[test]
    fn a_worktree_only_moves_and_stays_on_its_list_wherever_the_pointer_goes() {
        let hand = TabHand::default();
        // Three worktrees down the sidebar, the middle one open on its tabs.
        let slots = [(0., 36.), (36., 112.), (148., 36.)].map(|(top, height)| Bounds {
            origin: point(px(0.), px(top)),
            size: size(px(240.), px(height)),
        });
        let list = List {
            id: 9,
            holds: Holds::Worktrees,
            axis: Axis::Vertical,
            keys: vec![1, 2, 3],
            bounds: Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(240.), px(184.)),
            },
        };
        hand.take(9, 1, 0, px(18.));
        let place = |x, y| hand.place(&list, &slots, at(x, y), true).unwrap();
        let held = place(120., 70.);
        assert_eq!(
            (held.to, held.join),
            (0, None),
            "over the middle: no joining"
        );
        assert_eq!(place(120., 100.).to, 1, "past the open one's middle");
        let held = place(120., 400.);
        assert!(!held.off, "past the list's end it's still on it");
        assert_eq!((held.to, held.pos), (2, px(184. - 36.)), "last, at the end");
        assert!(!place(900., 400.).off, "nor taken off sideways");
        assert!(!hand.chip_shows());
        assert_eq!(hand.release(9), Some(Drop::Move { from: 0, to: 2 }));
    }

    #[test]
    fn a_list_leaves_another_lists_tab_alone_and_lets_go_once_the_drag_is_over() {
        let hand = TabHand::default();
        let (list, slots) = strip(2);
        hand.take(8, 500, 0, px(10.));
        assert!(hand.place(&list, &slots, at(50., 14.), true).is_none());
        assert_eq!(
            hand.holding(),
            Some(500),
            "the other list's tab stays in hand"
        );
        assert_eq!(hand.release(7), None);

        hand.take(7, 101, 1, px(10.));
        assert!(hand.place(&list, &slots, at(50., 14.), false).is_none());
        assert!(hand.holding().is_none(), "gpui's drag is over");
        assert!(hand.settling(7));
    }
}
