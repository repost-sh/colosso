//! Places (`docs/devices.md` 7): everything above the host gives points of the upright picture as
//! shown; a device takes shares of its own screen in its own axes. This is the one place the two
//! are turned into each other, and the GPU's turn (`gpu.rs`) is checked against it here.

use koyo_device_wire::Orientation;

/// How an interface faces when its picture is put upright by `quarters` clockwise: a simulator
/// draws a turned screen turned inside its one portrait surface, and says which way (`rot90` is
/// one quarter).
pub fn facing(quarters: u8) -> Orientation {
    match quarters % 4 {
        1 => Orientation::LandscapeRight,
        2 => Orientation::PortraitUpsideDown,
        3 => Orientation::LandscapeLeft,
        _ => Orientation::Portrait,
    }
}

/// The quarters clockwise that put the picture upright when the interface faces `orientation`:
/// [`facing`]'s other way.
pub fn upright(orientation: Orientation) -> u8 {
    (4 - orientation.quarters()) % 4
}

/// Where a share `(u, v)` of the upright picture is on the surface, as a share of it, when the
/// picture is the surface turned `quarters` clockwise.
pub fn on_surface(quarters: u8, (u, v): (f64, f64)) -> (f64, f64) {
    match quarters % 4 {
        1 => (v, 1. - u),
        2 => (1. - u, 1. - v),
        3 => (1. - v, u),
        _ => (u, v),
    }
}

/// Where a share `(x, y)` of a real device's upright picture goes in the device's own upright
/// axes, by how it is held: measured on an iPad on iPadOS 27.0.1 (POC.md, the ninth round).
pub fn on_device(held: Orientation, (x, y): (f64, f64)) -> (f64, f64) {
    match held {
        Orientation::LandscapeRight => (y, 1. - x),
        Orientation::Portrait => (x, y),
        Orientation::LandscapeLeft => (1. - y, x),
        Orientation::PortraitUpsideDown => (1. - x, 1. - y),
    }
}

/// A share of a screen, 0 to 1, as a device's touch reports carry it: 0 to 65535.
pub fn share(value: f64) -> u16 {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=65535 first")]
    let share = (value.clamp(0., 1.) * 65535.).round() as u16;
    share
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kernel's own rule, in pixels: the slot's pixel at `p` comes from the source's at this.
    fn kernel(quarters: u8, (px, py): (u32, u32), (w, h): (u32, u32)) -> (u32, u32) {
        match quarters {
            1 => (py, w - 1 - px),
            2 => (w - 1 - px, h - 1 - py),
            3 => (h - 1 - py, px),
            _ => (px, py),
        }
    }

    #[test]
    fn upright_is_facing_turned_back() {
        for quarters in 0..4 {
            assert_eq!(upright(facing(quarters)), quarters);
        }
    }

    #[test]
    fn touches_land_where_the_turned_picture_shows_them() {
        // A surface 6 wide and 10 high, as a portrait simulator's is.
        let (sw, sh) = (6u32, 10u32);
        for quarters in 0..4u8 {
            let (w, h) = if quarters % 2 == 1 {
                (sh, sw)
            } else {
                (sw, sh)
            };
            for px in 0..w {
                for py in 0..h {
                    let (sx, sy) = kernel(quarters, (px, py), (w, h));
                    // The middle of the slot's pixel, as a share, mapped to the surface…
                    let share = (
                        (f64::from(px) + 0.5) / f64::from(w),
                        (f64::from(py) + 0.5) / f64::from(h),
                    );
                    let (x, y) = on_surface(quarters, share);
                    // …is the middle of the pixel the kernel took it from.
                    assert!(
                        (x * f64::from(sw) - (f64::from(sx) + 0.5)).abs() < 1e-9,
                        "{quarters} {px},{py}"
                    );
                    assert!(
                        (y * f64::from(sh) - (f64::from(sy) + 0.5)).abs() < 1e-9,
                        "{quarters} {px},{py}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_turn_is_undone_by_the_one_opposite() {
        let at = (0.2, 0.7);
        for quarters in 0..4u8 {
            let back = on_surface((4 - quarters) % 4, on_surface(quarters, at));
            assert!(
                (back.0 - at.0).abs() < 1e-12 && (back.1 - at.1).abs() < 1e-12,
                "{quarters}"
            );
        }
        assert_eq!(facing(0), Orientation::Portrait);
        assert_eq!(facing(3), Orientation::LandscapeLeft);
    }

    #[test]
    fn a_held_ipad_takes_places_as_measured() {
        // The ninth round's table: a place (x, y) of the picture, to the device.
        assert_eq!(on_device(Orientation::LandscapeRight, (0.25, 0.75)), (0.75, 0.75));
        assert_eq!(on_device(Orientation::Portrait, (0.25, 0.75)), (0.25, 0.75));
        assert_eq!(on_device(Orientation::LandscapeLeft, (0.25, 0.75)), (0.25, 0.25));
        assert_eq!(
            on_device(Orientation::PortraitUpsideDown, (0.25, 0.75)),
            (0.75, 0.25)
        );
        assert_eq!((share(0.), share(1.), share(2.)), (0, 65535, 65535));
    }
}
