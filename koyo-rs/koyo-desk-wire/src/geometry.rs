//! Shapes on the agent desktop, in points from its top left, as `CoreGraphics` has the display.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.y >= self.y
            && point.x < self.x + self.width
            && point.y < self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }

    /// `self` moved and, if it must be, shrunk to lie inside `outer`.
    #[must_use]
    pub fn kept_inside(&self, outer: Rect) -> Rect {
        let width = self.width.min(outer.width);
        let height = self.height.min(outer.height);
        Rect {
            x: self.x.clamp(outer.x, outer.x + outer.width - width),
            y: self.y.clamp(outer.y, outer.y + outer.height - height),
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1800.0,
        height: 1169.0,
    };

    #[test]
    fn a_point_on_the_far_edge_is_outside() {
        assert!(DESKTOP.contains(Point { x: 0.0, y: 0.0 }));
        assert!(DESKTOP.contains(Point {
            x: 1799.5,
            y: 1168.5
        }));
        assert!(!DESKTOP.contains(Point { x: 1800.0, y: 10.0 }));
        assert!(!DESKTOP.contains(Point { x: -1.0, y: 10.0 }));
    }

    #[test]
    fn a_window_is_moved_inside_and_shrunk_only_when_it_must_be() {
        let hanging = Rect {
            x: 1500.0,
            y: -40.0,
            width: 600.0,
            height: 400.0,
        };
        assert_eq!(
            hanging.kept_inside(DESKTOP),
            Rect {
                x: 1200.0,
                y: 0.0,
                width: 600.0,
                height: 400.0
            }
        );
        let huge = Rect {
            x: 10.0,
            y: 10.0,
            width: 4000.0,
            height: 3000.0,
        };
        assert_eq!(huge.kept_inside(DESKTOP), DESKTOP);
        let fine = Rect {
            x: 100.0,
            y: 60.0,
            width: 900.0,
            height: 600.0,
        };
        assert_eq!(fine.kept_inside(DESKTOP), fine);
    }
}
