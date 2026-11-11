//! Text read off the picture (`docs/devices.md` 6), with macOS's own Vision framework: public, on
//! the Mac, nothing sent anywhere. A snapshot's `source: text`, which is all a real device has
//! and what a simulator falls back to. Pieces under 0.9 confidence are noise (an icon's drawing,
//! the battery) and are left out. Measured in the proof of concept: 16 of 16 pieces of a Settings screen,
//! 74-125 ms.

use std::fmt::Write as _;

use objc2::msg_send;
use objc2::rc::Allocated;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::CGRect;
use objc2_core_video::CVPixelBuffer;
use objc2_foundation::{NSArray, NSDictionary, NSError, NSString};
use objc2_io_surface::IOSurfaceRef;

use crate::sim::core::guarded;
use crate::sys;
use koyo_device_wire::Rect;

const VISION: &std::ffi::CStr = c"/System/Library/Frameworks/Vision.framework/Vision";
const NOISE: f32 = 0.9;

/// Whether Vision has read once in this process. The first read of a new build of the host has
/// macOS compile its text model, which took 30-40 s here (and once 39.6 s in the proof of
/// concept): it is done as the host starts, while the simulator boots, and a snapshot asked
/// before it's done is told to ask again rather than left past its time.
static WARM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn warm() -> bool {
    WARM.load(std::sync::atomic::Ordering::Relaxed)
}

/// Reads a small blank picture once, on a thread of its own, so the first real read is quick.
pub fn warm_up() {
    std::thread::spawn(|| {
        objc2::rc::autoreleasepool(|_| {
            if let Some(blank) = crate::ring::surface(256, 64) {
                let _ = read(&blank, 1.);
            }
        });
        WARM.store(true, std::sync::atomic::Ordering::Relaxed);
    });
}

/// A piece of text and where it is, in points of the upright picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub frame: Rect,
}

/// The text on `surface`, whose pixels are `scale` a point, top to bottom.
pub fn read(surface: &IOSurfaceRef, scale: f64) -> Result<Vec<Piece>, String> {
    if !sys::load(VISION) {
        return Err("macOS's Vision framework didn't load".into());
    }
    let (width, height) = (
        surface.width() as f64 / scale,
        surface.height() as f64 / scale,
    );
    let buffer = crate::ring::pixel_buffer(surface).ok_or("no pixel buffer over the picture")?;
    let pieces = guarded("reading the picture's text", || recognize(&buffer))??;
    let mut placed: Vec<Piece> = pieces
        .into_iter()
        .filter(|(_, confidence, _)| *confidence >= NOISE)
        .map(|(text, _, frame)| Piece {
            text,
            // Vision's boxes are shares of the picture from its bottom left.
            frame: Rect {
                x: frame.origin.x * width,
                y: (1. - frame.origin.y - frame.size.height) * height,
                width: frame.size.width * width,
                height: frame.size.height * height,
            },
        })
        .collect();
    placed.sort_by(|a, b| {
        (a.frame.y.round(), a.frame.x)
            .partial_cmp(&(b.frame.y.round(), b.frame.x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(placed)
}

type Found = Vec<(String, f32, CGRect)>;

fn recognize(buffer: &CVPixelBuffer) -> Result<Found, String> {
    let request_class =
        AnyClass::get(c"VNRecognizeTextRequest").ok_or("no VNRecognizeTextRequest")?;
    let handler_class =
        AnyClass::get(c"VNImageRequestHandler").ok_or("no VNImageRequestHandler")?;
    // SAFETY: Vision's public classes and methods, with the types its headers give.
    unsafe {
        let request: Allocated<AnyObject> = msg_send![request_class, alloc];
        let request: Retained<AnyObject> = msg_send![request, init];
        // VNRequestTextRecognitionLevelAccurate.
        let () = msg_send![&request, setRecognitionLevel: 0isize];
        let () = msg_send![&request, setUsesLanguageCorrection: true];
        let options = NSDictionary::<NSString, AnyObject>::new();
        let handler: Allocated<AnyObject> = msg_send![handler_class, alloc];
        let handler: Retained<AnyObject> =
            msg_send![handler, initWithCVPixelBuffer: buffer, options: &*options];
        let requests = NSArray::from_retained_slice(std::slice::from_ref(&request));
        let done: Result<(), Retained<NSError>> =
            msg_send![&handler, performRequests: &*requests, error: _];
        done.map_err(|error| format!("Vision refused: {error}"))?;
        let results: Option<Retained<NSArray<AnyObject>>> = msg_send![&request, results];
        let mut found = Vec::new();
        for observation in results.iter().flat_map(|all| all.iter()) {
            let candidates: Retained<NSArray<AnyObject>> =
                msg_send![&observation, topCandidates: 1usize];
            let Some(best) = candidates.firstObject() else {
                continue;
            };
            let text: Retained<NSString> = msg_send![&best, string];
            let confidence: f32 = msg_send![&best, confidence];
            let frame: CGRect = msg_send![&observation, boundingBox];
            found.push((text.to_string(), confidence, frame));
        }
        Ok(found)
    }
}

/// The pieces as a snapshot's tree, numbered from `first`: each on a line with its ref and the
/// middle of its box, at most `cap` of them.
pub fn tree(
    pieces: &[Piece],
    first: u32,
    cap: usize,
) -> (String, Vec<(String, koyo_device_wire::Point)>, bool) {
    let mut lines = String::new();
    let mut refs = Vec::new();
    for (piece, number) in pieces.iter().take(cap).zip(first..) {
        let middle = koyo_device_wire::Point {
            x: piece.frame.x + piece.frame.width / 2.,
            y: piece.frame.y + piece.frame.height / 2.,
        };
        let name = format!("e{number}");
        let _ = writeln!(
            lines,
            "text {:?} [ref={name}] @{:.0},{:.0}",
            piece.text, middle.x, middle.y
        );
        refs.push((name, middle));
    }
    (lines, refs, pieces.len() > cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_read_as_lines_with_refs_at_their_middles() {
        let piece = |text: &str, y| Piece {
            text: text.into(),
            frame: Rect {
                x: 20.,
                y,
                width: 100.,
                height: 20.,
            },
        };
        let (tree, refs, truncated) =
            tree(&[piece("Settings", 50.), piece("General", 300.)], 7, 10);
        assert_eq!(
            tree,
            "text \"Settings\" [ref=e7] @70,60\ntext \"General\" [ref=e8] @70,310\n"
        );
        assert_eq!(refs[1].0, "e8");
        assert!(!truncated);
        let (_, refs, truncated) = super::tree(&[piece("a", 0.), piece("b", 1.)], 1, 1);
        assert_eq!(refs.len(), 1);
        assert!(truncated);
    }
}
