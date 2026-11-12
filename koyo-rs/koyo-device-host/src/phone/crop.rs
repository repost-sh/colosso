//! The screen within a real device's pictures (`docs/devices.md` 5). A picture is coded larger
//! than the screen it shows: the screen in its top left, the rest black (an iPhone 14 Pro's
//! 1179 x 2556 in 1184 x 2576), and nothing in the stream says so. The device says its screen's
//! size; no more than it is shown of a picture. That the rest is black is looked at on a
//! picture that is lit, and should it not be, the crop is given up and the picture shown whole:
//! a device that codes its pictures another way is shown with a black edge, never cut.

use koyo_device_wire::PixelSize;
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString};
use objc2_core_video::{
    CVAttachmentMode, CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeightOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVImageBufferCleanApertureHeightKey,
    kCVImageBufferCleanApertureHorizontalOffsetKey, kCVImageBufferCleanApertureKey, kCVImageBufferCleanApertureVerticalOffsetKey,
    kCVImageBufferCleanApertureWidthKey,
};

/// How wide the screen's own edge is looked at, in rows and columns, beside what is cut.
const EDGE: usize = 16;
/// The screen's edge is lit from this light on, of 255; what is cut is black up to this (a
/// coder's black is 0, or 16 in video range).
const LIT: f64 = 48.;
const BLACK: f64 = 20.;
/// Until a lit picture has been looked at, one in this many is.
const LOOKS_EVERY: u32 = 30;

/// A picture's clean aperture: its size, and where its middle is from the picture's.
type Aperture = CFRetained<CFDictionary<CFString, CFNumber>>;

/// Whether what is cut from pictures has been seen to be black.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    NotYet,
    Black,
    /// Something was there: the pictures are shown whole from then on.
    Lit,
}

pub struct Crop {
    screen: Option<(usize, usize)>,
    seen: Seen,
    pictures: u32,
    /// The clean aperture for pictures coded at this size, made once.
    aperture: Option<((usize, usize), Aperture)>,
}

impl Crop {
    /// For a device whose screen is `screen` pixels, as it said; with `None` nothing is cut.
    pub fn new(screen: Option<PixelSize>) -> Crop {
        let side = |pixels: u32| usize::try_from(pixels).ok().filter(|pixels| *pixels > 0);
        Crop {
            screen: screen.and_then(|screen| Some((side(screen.width)?, side(screen.height)?))),
            seen: Seen::NotYet,
            pictures: 0,
            aperture: None,
        }
    }

    /// How much of `image`, coded as `coded`, is shown: the screen, marked on the picture as its
    /// clean aperture for the transfer to crop to, or all of it. `log` hears, once, why a crop
    /// was given up.
    pub fn of(&mut self, image: &CVPixelBuffer, coded: (usize, usize), log: impl Fn(&str)) -> (usize, usize) {
        let Some(screen) = self.screen.filter(|screen| *screen != coded) else {
            return coded;
        };
        if screen.0 > coded.0 || screen.1 > coded.1 {
            log(&format!("the picture is coded {}x{}, smaller than the screen's {}x{}: shown whole", coded.0, coded.1, screen.0, screen.1));
            self.screen = None;
            return coded;
        }
        if self.seen == Seen::NotYet && self.pictures.is_multiple_of(LOOKS_EVERY) {
            self.seen = plane(image, coded).map_or(Seen::NotYet, |(light, stride)| seen(&light, stride, coded, screen));
            if self.seen == Seen::Lit {
                log(&format!(
                    "the picture is coded {}x{} and its screen is {}x{}, but what that would cut away isn't black: shown whole",
                    coded.0, coded.1, screen.0, screen.1
                ));
            }
        }
        self.pictures = self.pictures.wrapping_add(1);
        if self.seen == Seen::Lit {
            return coded;
        }
        if self.aperture.as_ref().is_none_or(|(of, _)| *of != coded) {
            self.aperture = Some((coded, aperture(coded, screen)));
        }
        if let Some((_, aperture)) = &self.aperture {
            // SAFETY: CoreVideo's own key, and under it the dictionary of numbers it asks for.
            unsafe { image.set_attachment(kCVImageBufferCleanApertureKey, aperture.as_ref(), CVAttachmentMode::ShouldNotPropagate) };
        }
        screen
    }
}

/// The top left `screen` of a picture coded as `coded`, as its clean aperture.
fn aperture(coded: (usize, usize), screen: (usize, usize)) -> Aperture {
    let from_middle = |screen: usize, coded: usize| CFNumber::new_f64((screen as f64 - coded as f64) / 2.);
    let (width, height) = (CFNumber::new_f64(screen.0 as f64), CFNumber::new_f64(screen.1 as f64));
    let (across, down) = (from_middle(screen.0, coded.0), from_middle(screen.1, coded.1));
    // SAFETY: CoreVideo's own keys, statics of the framework.
    let keys: [&CFString; 4] = unsafe {
        [
            kCVImageBufferCleanApertureWidthKey,
            kCVImageBufferCleanApertureHeightKey,
            kCVImageBufferCleanApertureHorizontalOffsetKey,
            kCVImageBufferCleanApertureVerticalOffsetKey,
        ]
    };
    CFDictionary::from_slices(&keys, &[&*width, &*height, &*across, &*down])
}

/// The picture's light, a byte a pixel, copied out with its bytes a row; nothing for a picture
/// that isn't in planes with its light first, a byte a pixel.
fn plane(image: &CVPixelBuffer, coded: (usize, usize)) -> Option<(Vec<u8>, usize)> {
    // SAFETY: the buffer is locked for reading while its first plane is copied, whole rows of it.
    unsafe {
        CVPixelBufferLockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly);
        let (base, stride) = (CVPixelBufferGetBaseAddressOfPlane(image, 0).cast::<u8>(), CVPixelBufferGetBytesPerRowOfPlane(image, 0));
        let whole = !base.is_null() && stride >= coded.0 && CVPixelBufferGetHeightOfPlane(image, 0) >= coded.1;
        let light = whole.then(|| std::slice::from_raw_parts(base, stride * coded.1).to_vec());
        CVPixelBufferUnlockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly);
        Some((light?, stride))
    }
}

/// What a picture says of the crop: `light` is its light, `stride` bytes a row. Black where it
/// would be cut, beside a screen lit at that edge; lit there; or too dark at the edge to say.
fn seen(light: &[u8], stride: usize, coded: (usize, usize), screen: (usize, usize)) -> Seen {
    // The mean light of the rows `rows` between the columns `columns`.
    let mean = |columns: std::ops::Range<usize>, rows: std::ops::Range<usize>| {
        let count = columns.len() * rows.len();
        let sum: u64 = rows.map(|y| light[y * stride + columns.start..y * stride + columns.end].iter().map(|byte| u64::from(*byte)).sum::<u64>()).sum();
        (count > 0).then(|| sum as f64 / count as f64)
    };
    // What is cut, to the right of the screen and under it; and the screen's own edge beside each.
    let cut = [mean(screen.0..coded.0, 0..screen.1), mean(0..coded.0, screen.1..coded.1)];
    let edge = [
        mean(screen.0.saturating_sub(EDGE)..screen.0, 0..screen.1).filter(|_| coded.0 > screen.0),
        mean(0..screen.0, screen.1.saturating_sub(EDGE)..screen.1).filter(|_| coded.1 > screen.1),
    ];
    if cut.iter().flatten().any(|light| *light > BLACK) {
        Seen::Lit
    } else if edge.iter().flatten().all(|light| *light >= LIT) {
        Seen::Black
    } else {
        Seen::NotYet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture of 40 x 60 with `lit` (columns, rows) at light 200 and the rest black.
    fn picture(columns: std::ops::Range<usize>, rows: std::ops::Range<usize>) -> Vec<u8> {
        let mut light = vec![0; 48 * 60];
        for y in rows {
            light[y * 48 + columns.start..y * 48 + columns.end].fill(200);
        }
        light
    }

    #[test]
    fn a_lit_screen_in_the_top_left_with_black_beside_it_is_cropped() {
        assert_eq!(seen(&picture(0..32, 0..50), 48, (40, 60), (32, 50)), Seen::Black);
    }

    #[test]
    fn a_screen_that_reaches_into_what_would_be_cut_is_shown_whole() {
        // In the middle of the picture, as a device that pads on every side would code it.
        assert_eq!(seen(&picture(4..36, 5..55), 48, (40, 60), (32, 50)), Seen::Lit);
    }

    #[test]
    fn a_dark_picture_says_nothing_yet() {
        assert_eq!(seen(&picture(0..0, 0..0), 48, (40, 60), (32, 50)), Seen::NotYet);
        // Lit, but dark at its foot: the foot's cut can't be told from the screen.
        assert_eq!(seen(&picture(0..32, 0..20), 48, (40, 60), (32, 50)), Seen::NotYet);
    }
}
