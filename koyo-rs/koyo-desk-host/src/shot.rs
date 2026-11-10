//! Screenshots (`docs/agent-desktop.md` 10): a window, or the whole desktop put together from
//! its windows (a display-wide capture gives nothing for a desktop that isn't showing), as PNG.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};

use koyo_desk_wire::link::{Answer, Refusal, RefusalCode, RequestId, Window};
use koyo_desk_wire::{Point, Rect, Size};

use crate::ns::{self, Obj};
use crate::sys;

/// How long a screenshot may take.
const WITHIN: Duration = Duration::from_secs(5);
/// `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`: BGRA, as the captures are.
const BGRA: u32 = 2 | (2 << 12);

/// An object handed once from a completion queue to the main thread.
pub struct Handed(Obj);
// SAFETY: shareable content and images are immutable once made, and only passed on.
unsafe impl Send for Handed {}
pub type Slot = Arc<Mutex<Option<Handed>>>;

pub fn take(slot: &Slot) -> Option<Obj> {
    (slot.lock().unwrap_or_else(PoisonError::into_inner).take()).map(|handed| handed.0)
}

/// The windows `content` (shareable content) lists, and the one among such with this number.
pub fn shareable(content: &AnyObject) -> Vec<Obj> {
    // SAFETY: shareable content answers `windows` with an array of its windows.
    let windows: Obj = unsafe { msg_send![content, windows] };
    ns::items(&windows)
}

pub fn numbered(shareable: &[Obj], id: u32) -> Option<&Obj> {
    shareable.iter().find(|candidate| {
        // SAFETY: a shareable window answers `windowID`.
        let number: u32 = unsafe { msg_send![&***candidate, windowID] };
        number == id
    })
}

/// A block that puts what it's handed into `slot` and wakes the main thread.
pub fn into(slot: &Slot) -> RcBlock<dyn Fn(*mut AnyObject, *mut AnyObject)> {
    let slot = Arc::clone(slot);
    RcBlock::new(move |object: *mut AnyObject, _error: *mut AnyObject| {
        // SAFETY: an object or nil; a `CGImage` is an object too.
        if let Some(object) = unsafe { Retained::retain(object) } {
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(Handed(object));
        }
        ns::wake();
    })
}

enum Stage {
    /// Waiting for what can be captured.
    Listing(Slot),
    /// Waiting for each window's picture, front to back.
    Capturing(Vec<(Rect, Slot, Option<Obj>)>),
}

struct Shot {
    request: RequestId,
    windows: Vec<Window>,
    /// What the picture covers, among the displays, and its top left as the app counts it,
    /// from the desktop's.
    area: Rect,
    origin: Point,
    scale: u8,
    started: Instant,
    stage: Stage,
}

/// The screenshots under way.
#[derive(Default)]
pub struct Shots {
    shots: Vec<Shot>,
}

impl Shots {
    /// Starts a picture of `windows` (front to back) over `area`, which the app knows by
    /// `origin`.
    pub fn ask(
        &mut self,
        request: RequestId,
        windows: Vec<Window>,
        (area, origin): (Rect, Point),
        scale: u8,
    ) {
        let listed: Slot = Arc::default();
        // SAFETY: the class's own listing call, whose block the callee copies.
        unsafe {
            let _: () = msg_send![
                class!(SCShareableContent),
                getShareableContentExcludingDesktopWindows: true,
                onScreenWindowsOnly: false,
                completionHandler: &*into(&listed)
            ];
        }
        self.shots.push(Shot {
            request,
            windows,
            area,
            origin,
            scale: scale.clamp(1, 2),
            started: Instant::now(),
            stage: Stage::Listing(listed),
        });
    }

    /// Drops the screenshot of a caller that went away.
    pub fn cancel(&mut self, request: RequestId) {
        self.shots.retain(|shot| shot.request != request);
    }

    /// The screenshots that are done, each with its answer.
    pub fn poll(&mut self) -> Vec<(RequestId, Result<Answer, Refusal>)> {
        let mut done = Vec::new();
        self.shots.retain_mut(|shot| match shot.advance() {
            Some(answer) => {
                done.push((shot.request, answer));
                false
            }
            None => true,
        });
        done
    }
}

impl Shot {
    fn advance(&mut self) -> Option<Result<Answer, Refusal>> {
        if self.started.elapsed() > WITHIN {
            return Some(Err(Refusal::new(
                RefusalCode::TimedOut,
                "the screenshot took too long: is Screen Recording granted?",
            )));
        }
        match &mut self.stage {
            Stage::Listing(listed) => {
                let content = take(listed)?;
                let shareable = shareable(&content);
                let scale = usize::from(self.scale);
                let parts = (self.windows.iter())
                    .filter_map(|window| {
                        let found = numbered(&shareable, window.id)?;
                        let slot: Slot = Arc::default();
                        capture(found, window.frame, scale, &slot);
                        Some((window.frame, slot, None))
                    })
                    .collect();
                self.stage = Stage::Capturing(parts);
                None
            }
            Stage::Capturing(parts) => {
                for (_, slot, image) in parts.iter_mut() {
                    if image.is_none() {
                        *image = take(slot);
                    }
                }
                if parts.iter().any(|(_, _, image)| image.is_none()) {
                    return None;
                }
                let png = compose(parts, self.area, f64::from(self.scale));
                Some(png.map_or_else(
                    || {
                        Err(Refusal::new(
                            RefusalCode::Failed,
                            "the picture couldn't be made",
                        ))
                    },
                    |png| {
                        Ok(Answer::Screenshot {
                            png: png.into(),
                            origin: self.origin,
                            size: Size {
                                width: self.area.width,
                                height: self.area.height,
                            },
                            scale: self.scale,
                        })
                    },
                ))
            }
        }
    }
}

/// Asks for one window's picture; it comes to `slot`.
fn capture(shareable: &Obj, frame: Rect, scale: usize, slot: &Slot) {
    // SAFETY: `ScreenCaptureKit`'s own constructors, setters and screenshot call, whose block
    // the callee copies.
    unsafe {
        let filter: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
        let filter: Obj = msg_send![filter, initWithDesktopIndependentWindow: &**shareable];
        let configuration: Obj = msg_send![class!(SCStreamConfiguration), new];
        let _: () = msg_send![&*configuration, setWidth: frame.width as usize * scale];
        let _: () = msg_send![&*configuration, setHeight: frame.height as usize * scale];
        let _: () = msg_send![&*configuration, setShowsCursor: false];
        crate::capture::alone(&configuration);
        crate::capture::srgb(&configuration);
        let _: () = msg_send![
            class!(SCScreenshotManager),
            captureImageWithFilter: &*filter,
            configuration: &*configuration,
            completionHandler: &*into(slot)
        ];
    }
}

/// The windows' pictures (front to back) drawn at their places over `area`, as PNG.
fn compose(parts: &[(Rect, Slot, Option<Obj>)], area: Rect, scale: f64) -> Option<Vec<u8>> {
    let (width, height) = (
        (area.width * scale).round() as usize,
        (area.height * scale).round() as usize,
    );
    // SAFETY: a bitmap context of that size in device RGB, which Core Graphics allocates and
    // we release; images drawn into rectangles inside it; the result encoded into data we own.
    unsafe {
        let space = ns::owned(sys::CGColorSpaceCreateDeviceRGB())?;
        let context = ns::owned(sys::CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            0,
            ns::raw(&space),
            BGRA,
        ))?;
        let whole = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: width as f64,
                height: height as f64,
            },
        };
        sys::CGContextSetRGBFillColor(ns::raw(&context), 0.11, 0.11, 0.12, 1.0);
        sys::CGContextFillRect(ns::raw(&context), whole);
        for (frame, _, image) in parts.iter().rev() {
            let Some(image) = image else { continue };
            // Core Graphics counts from the bottom left.
            let rect = CGRect {
                origin: CGPoint {
                    x: (frame.x - area.x) * scale,
                    y: (area.height - (frame.y - area.y) - frame.height) * scale,
                },
                size: CGSize {
                    width: frame.width * scale,
                    height: frame.height * scale,
                },
            };
            sys::CGContextDrawImage(ns::raw(&context), rect, ns::raw(image));
        }
        let picture = ns::owned(sys::CGBitmapContextCreateImage(ns::raw(&context)))?;
        let data = ns::owned(sys::CFDataCreateMutable(std::ptr::null(), 0))?;
        let destination = ns::owned(sys::CGImageDestinationCreateWithData(
            ns::raw(&data),
            ns::raw(&ns::string("public.png")),
            1,
            std::ptr::null(),
        ))?;
        sys::CGImageDestinationAddImage(ns::raw(&destination), ns::raw(&picture), std::ptr::null());
        if !sys::CGImageDestinationFinalize(ns::raw(&destination)) {
            return None;
        }
        let length = usize::try_from(sys::CFDataGetLength(ns::raw(&data))).ok()?;
        Some(std::slice::from_raw_parts(sys::CFDataGetBytePtr(ns::raw(&data)), length).to_vec())
    }
}
