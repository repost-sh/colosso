//! Where a wallpaper lies on its screen, as macOS lays it out from the options the desktop
//! picture was set with (`NSWorkspace.desktopImageOptions(for:)`): a function of the picture's
//! size, the screen's and those options, with nothing of `AppKit` in it.

/// How a wallpaper is laid out on its screen, as System Settings names it. Tiling isn't among
/// the options `AppKit` gives, and isn't handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scaling {
    /// Fill Screen: scaled to cover the screen, centred, what reaches past it cut off.
    Fill,
    /// Fit to Screen: scaled to fit, centred, the fill colour round it.
    Fit,
    /// Stretch to Fill Screen: stretched to the screen.
    Stretch,
    /// Centre: at its own size in points, centred, the fill colour round it.
    Centre,
    /// As `Fit`, but never larger than its own size.
    FitDown,
}

impl Scaling {
    /// The layout an `NSImageScaling` of `raw` names, with clipping `allowed` or not. A value
    /// `AppKit` doesn't define is taken as Fill Screen, its default for a picture.
    pub fn from_options(raw: usize, allowed: bool) -> Scaling {
        match (raw, allowed) {
            (0, _) => Scaling::FitDown,
            (1, _) => Scaling::Stretch,
            (2, _) => Scaling::Centre,
            (3, false) => Scaling::Fit,
            _ => Scaling::Fill,
        }
    }
}

/// A rectangle in a screen's points, from its top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    /// Whether it covers the whole of a screen `screen` points in size, leaving nothing for the
    /// fill colour.
    pub fn covers(&self, screen: (f64, f64)) -> bool {
        const SLACK: f64 = 0.01;
        self.x <= SLACK
            && self.y <= SLACK
            && self.x + self.width >= screen.0 - SLACK
            && self.y + self.height >= screen.1 - SLACK
    }
}

/// Where a picture `picture` points in size lies on a screen `screen` points in size, laid out
/// as `scaling` says. It may reach past the screen, which shows only its own part of it.
pub fn place(picture: (f64, f64), screen: (f64, f64), scaling: Scaling) -> Rect {
    let (width, height) = (
        picture.0.max(f64::MIN_POSITIVE),
        picture.1.max(f64::MIN_POSITIVE),
    );
    let fit = (screen.0 / width).min(screen.1 / height);
    let size = match scaling {
        Scaling::Stretch => return centred(screen, screen),
        Scaling::Fill => {
            let cover = (screen.0 / width).max(screen.1 / height);
            (width * cover, height * cover)
        }
        Scaling::Fit => (width * fit, height * fit),
        Scaling::Centre => (width, height),
        Scaling::FitDown => (width * fit.min(1.), height * fit.min(1.)),
    };
    centred(size, screen)
}

/// A rectangle `size` points in size, centred on a screen `screen` points in size.
fn centred(size: (f64, f64), screen: (f64, f64)) -> Rect {
    Rect {
        x: (screen.0 - size.0) / 2.,
        y: (screen.1 - size.1) / 2.,
        width: size.0,
        height: size.1,
    }
}

/// How many pixels the longest side of a picture `pixels` in size is decoded at, to be drawn
/// over `placed` on a screen of `scale` pixels to the point: as many as that side spans on
/// the screen, never more than the picture has.
pub fn decode_longest(pixels: (u32, u32), placed: Rect, scale: f64) -> u32 {
    let (longest, spans) = if pixels.0 >= pixels.1 {
        (pixels.0, placed.width)
    } else {
        (pixels.1, placed.height)
    };
    let spans = (spans * scale).ceil().max(1.);
    if spans >= f64::from(longest) {
        longest
    } else {
        spans as u32
    }
}

#[cfg(test)]
mod tests {
    use super::{Rect, Scaling, decode_longest, place};

    const SCREEN: (f64, f64) = (1600., 900.);

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn assert_near(placed: Rect, expected: Rect) {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(
            near(placed.x, expected.x)
                && near(placed.y, expected.y)
                && near(placed.width, expected.width)
                && near(placed.height, expected.height),
            "{placed:?} isn't {expected:?}"
        );
    }

    #[test]
    fn the_options_name_the_layouts_system_settings_offers() {
        assert_eq!(Scaling::from_options(3, true), Scaling::Fill);
        assert_eq!(Scaling::from_options(3, false), Scaling::Fit);
        assert_eq!(Scaling::from_options(1, true), Scaling::Stretch);
        assert_eq!(Scaling::from_options(1, false), Scaling::Stretch);
        assert_eq!(Scaling::from_options(2, false), Scaling::Centre);
        assert_eq!(Scaling::from_options(0, false), Scaling::FitDown);
        assert_eq!(Scaling::from_options(9, false), Scaling::Fill, "unknown");
    }

    #[test]
    fn fill_covers_the_screen_centred_and_cuts_off_the_rest() {
        let wide = place((3000., 1000.), SCREEN, Scaling::Fill);
        assert_near(wide, rect(-550., 0., 2700., 900.));
        assert!(wide.covers(SCREEN));
        let tall = place((1000., 1000.), SCREEN, Scaling::Fill);
        assert_near(tall, rect(0., -350., 1600., 1600.));
        assert!(tall.covers(SCREEN));
        let small = place((160., 90.), SCREEN, Scaling::Fill);
        assert_near(small, rect(0., 0., 1600., 900.));
    }

    #[test]
    fn fit_shows_it_whole_centred_with_the_fill_colour_round_it() {
        let wide = place((3000., 1000.), SCREEN, Scaling::Fit);
        assert_near(wide, rect(0., 450. - 1600. / 6., 1600., 1600. / 3.));
        assert!(!wide.covers(SCREEN));
        let tall = place((1000., 2000.), SCREEN, Scaling::Fit);
        assert_near(tall, rect(575., 0., 450., 900.));
        assert!(!tall.covers(SCREEN));
        let same = place((3200., 1800.), SCREEN, Scaling::Fit);
        assert!(
            same.covers(SCREEN),
            "a picture of the screen's shape fits it exactly"
        );
    }

    #[test]
    fn stretch_fills_the_screen_whatever_its_shape() {
        let placed = place((1000., 3000.), SCREEN, Scaling::Stretch);
        assert_near(placed, rect(0., 0., 1600., 900.));
        assert!(placed.covers(SCREEN));
    }

    #[test]
    fn centre_keeps_its_own_size() {
        let small = place((800., 600.), SCREEN, Scaling::Centre);
        assert_near(small, rect(400., 150., 800., 600.));
        assert!(!small.covers(SCREEN));
        let large = place((3200., 1800.), SCREEN, Scaling::Centre);
        assert_near(large, rect(-800., -450., 3200., 1800.));
        assert!(large.covers(SCREEN));
    }

    #[test]
    fn fit_down_shrinks_a_large_picture_and_never_enlarges_a_small_one() {
        let small = place((800., 600.), SCREEN, Scaling::FitDown);
        assert_near(small, rect(400., 150., 800., 600.));
        let large = place((3000., 1000.), SCREEN, Scaling::FitDown);
        assert_near(large, place((3000., 1000.), SCREEN, Scaling::Fit));
    }

    #[test]
    fn a_picture_is_decoded_at_the_pixels_it_spans_and_no_more_than_it_has() {
        let fill = place((6016., 3384.), (2560., 1440.), Scaling::Fill);
        assert_eq!(decode_longest((6016, 3384), fill, 2.), 5120);
        assert_eq!(decode_longest((6016, 3384), fill, 1.), 2560);
        let centred = place((800., 600.), (2560., 1440.), Scaling::Centre);
        assert_eq!(
            decode_longest((1600, 1200), centred, 2.),
            1600,
            "all it has"
        );
        let tall = place((1000., 3000.), (2560., 1440.), Scaling::Fill);
        assert_eq!(decode_longest((1000, 3000), tall, 2.), 3000, "all it has");
        let tall = place((3000., 9000.), (2560., 1440.), Scaling::Fit);
        assert_eq!(
            decode_longest((3000, 9000), tall, 2.),
            2880,
            "its height spans the screen's"
        );
    }
}
