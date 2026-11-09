//! Areas on the agent screen (`docs/agent-desktop.md` 6): 1680 × 1050 points each, at fixed
//! slots that never move, the screen as large as the slots in use need and never larger than
//! Screen Sharing's 3840 × 2160.

use serde::{Deserialize, Serialize};

/// A point on the agent screen, or in an area, in points from the top left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A size in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// Every area's size: 16:10 at 1x.
pub const AREA: Size = Size {
    width: 1680,
    height: 1050,
};

/// How many areas one agent screen holds: two by two on Screen Sharing's largest display.
pub const SLOTS: u8 = 4;

/// Columns of slots: slot 0 at the top left, 1 right of it, 2 below 0, 3 below 1.
const COLUMNS: u8 = 2;

/// The largest display Screen Sharing gives a session ("Agent screen ceiling" in FINDINGS).
pub const SCREEN_MAX: Size = Size {
    width: 3840,
    height: 2160,
};

/// Where slot `slot`'s area starts on the agent screen.
pub fn slot_origin(slot: u8) -> Point {
    let column = slot % COLUMNS;
    let row = slot / COLUMNS;
    Point {
        x: f64::from(column) * f64::from(AREA.width),
        y: f64::from(row) * f64::from(AREA.height),
    }
}

/// Which slots are in use, and so how large the agent screen must be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    used: u8,
}

impl Layout {
    /// The slots in `slots` in use; slots past [`SLOTS`] are ignored.
    pub fn of(slots: impl IntoIterator<Item = u8>) -> Layout {
        let mut layout = Layout::default();
        for slot in slots {
            layout.take(slot);
        }
        layout
    }

    /// The lowest free slot, so the screen grows right, then down.
    pub fn free(self) -> Option<u8> {
        (0..SLOTS).find(|slot| !self.has(*slot))
    }

    pub fn has(self, slot: u8) -> bool {
        slot < SLOTS && self.used & (1 << slot) != 0
    }

    /// Marks `slot` in use. Whether it was free.
    pub fn take(&mut self, slot: u8) -> bool {
        if slot >= SLOTS || self.has(slot) {
            return false;
        }
        self.used |= 1 << slot;
        true
    }

    /// Frees `slot`. Whether it was in use.
    pub fn free_slot(&mut self, slot: u8) -> bool {
        if !self.has(slot) {
            return false;
        }
        self.used &= !(1 << slot);
        true
    }

    pub fn slots(self) -> impl Iterator<Item = u8> {
        (0..SLOTS).filter(move |slot| self.has(*slot))
    }

    pub fn len(self) -> usize {
        self.used.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.used == 0
    }

    /// The agent screen these slots need: as many columns and rows as the furthest slot in use,
    /// one area's size when none is (a session always has a display).
    pub fn screen(self) -> Size {
        let columns = self
            .slots()
            .map(|slot| slot % COLUMNS + 1)
            .max()
            .unwrap_or(1);
        let rows = self
            .slots()
            .map(|slot| slot / COLUMNS + 1)
            .max()
            .unwrap_or(1);
        Size {
            width: u32::from(columns) * AREA.width,
            height: u32::from(rows) * AREA.height,
        }
    }
}

#[cfg(test)]
mod tests;
