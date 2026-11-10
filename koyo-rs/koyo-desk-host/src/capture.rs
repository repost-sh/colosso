//! The picture (`docs/agent-desktop.md` 6): one `ScreenCaptureKit` stream per window on the
//! agent desktop (a display-wide capture delivers nothing for a desktop that isn't showing),
//! each frame's `IOSurface` sent to the app as a mach port, as it is: no codec, no copy.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use koyo_browser_ipc::mach;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, Encode, Encoding, class, define_class, msg_send};

use koyo_desk_wire::link::Window;

use crate::compare::{Change, Sampled};
use crate::ns::{self, Obj};
use crate::shot::{self, Slot};
use crate::sys;

/// Frames a second, at most, while a tab shows the desktop or a wait watches it: a window on
/// a desktop that isn't showing is captured at 60.
const RATE: i32 = 60;
/// And while nobody looks. A window on a desktop that isn't showing, captured for no one,
/// counts as hidden to its app, which then stops short of its work: a save panel never
/// turns its Save button on, a page isn't drawn. Two frames a second keep it seen.
const UNWATCHED_RATE: i32 = 2;
/// Frames whose buffers stay alive after they're sent, so the app draws a surface that isn't
/// being written to again.
const KEPT: usize = 3;
/// `'BGRA'`, one of the two formats the app's renderer draws.
const BGRA: u32 = 0x4247_5241;
/// A frame that changed no more than this, in square points, isn't the picture changing: a
/// text cursor blinking is a few points wide and a line high, and never rests.
const CURSOR: f64 = 200.0;

/// `CMTime`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Time {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

// SAFETY: the layout and encoding of `CMTime`, an anonymous struct of these four fields.
unsafe impl Encode for Time {
    const ENCODING: Encoding = Encoding::Struct(
        "?",
        &[
            Encoding::LongLong,
            Encoding::Int,
            Encoding::UInt,
            Encoding::LongLong,
        ],
    );
}

/// A pixel buffer kept alive on the capture queue.
struct Kept(Obj);
// SAFETY: a Core Video buffer, retained and released from any thread.
unsafe impl Send for Kept {}

struct Sink {
    window: u32,
    sender: Arc<mach::Sender>,
    sequence: AtomicU32,
    frames: Arc<AtomicU64>,
    /// Frames macOS delivered, sent on or not.
    delivered: Arc<AtomicU64>,
    /// Frames that changed more than a text cursor does.
    changes: Arc<AtomicU64>,
    /// Whether somebody looks: the app is sent frames only then.
    watched: Arc<AtomicBool>,
    /// Whether its changes count as the desktop changing: the wallpaper's don't.
    counts: bool,
    /// The pixels a point.
    scale: f64,
    last: Mutex<Sampled>,
    /// When a frame was last sent.
    sent: Mutex<Option<Instant>>,
    kept: Mutex<VecDeque<Kept>>,
    /// The frame that came last while nobody looked: a window that stays as it is, as a
    /// wallpaper does, sends no other.
    unseen: Mutex<Option<Kept>>,
}

/// A still window's frame is sent again this often: a frame lost on the way (the app busy,
/// its picture started over) would otherwise leave the window blank until it next changed.
const AGAIN_AFTER: Duration = Duration::from_secs(2);
/// How long a window's size has to stand before its stream is started again at that size.
const RESIZED_FOR: Duration = Duration::from_millis(150);
/// How long `ScreenCaptureKit` gets to list what can be captured before it's asked again.
const ASKS_AGAIN_AFTER: Duration = Duration::from_secs(3);

define_class!(
    /// Takes a stream's frames on the stream's own queue and sends each on.
    #[unsafe(super(NSObject))]
    #[name = "KoyoDeskStreamOutput"]
    #[ivars = Sink]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    impl Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn frame(&self, _stream: &AnyObject, sample: *mut c_void, _kind: isize) {
            self.ivars().send(sample.cast_const());
        }
    }
);

impl Sink {
    fn send(&self, sample: sys::Ref) {
        // SAFETY: a live sample buffer for the length of the callback; its image buffer and
        // that buffer's surface are lent, and kept or turned into a port before returning. An
        // idle frame has no image buffer.
        let (buffer, surface) = unsafe {
            let buffer = sys::CMSampleBufferGetImageBuffer(sample);
            if buffer.is_null() {
                return;
            }
            let surface = sys::CVPixelBufferGetIOSurface(buffer);
            if surface.is_null() {
                return;
            }
            (ns::kept(buffer), surface)
        };
        self.delivered.fetch_add(1, Ordering::Relaxed);
        // A window on a desktop that isn't showing is delivered whole 60 times a second
        // whether or not it changed: a frame equal to the last is dropped here, so a still
        // desktop costs the app nothing.
        let step = (self.scale.round() as usize * 2).max(1);
        let change = (self.last.lock().unwrap_or_else(PoisonError::into_inner)).compare(
            surface,
            step,
            CURSOR * self.scale * self.scale,
        );
        if change == Change::Big && self.counts {
            self.changes.fetch_add(1, Ordering::Relaxed);
        }
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.watched.load(Ordering::Relaxed) {
            // Nobody draws it: it's kept for when somebody does.
            *sent = None;
            *self.unseen.lock().unwrap_or_else(PoisonError::into_inner) = buffer.map(Kept);
            return;
        }
        if change == Change::Same && sent.is_some_and(|sent| sent.elapsed() < AGAIN_AFTER) {
            return;
        }
        *sent = Some(Instant::now());
        drop(sent);
        self.pass(surface, buffer);
    }

    /// Sends a frame's surface to the app, and keeps its buffer alive a while.
    fn pass(&self, surface: sys::Ref, buffer: Option<Obj>) {
        // SAFETY: a live surface; the right made is moved to the app below.
        let port = unsafe { sys::IOSurfaceCreateMachPort(surface) };
        if port == 0 {
            return;
        }
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        // The right moves to the app whether or not the send succeeds.
        if (self.sender)
            .send(u64::from(self.window), sequence, &[port])
            .is_ok()
        {
            self.frames.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(buffer) = buffer {
            let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
            kept.push_back(Kept(buffer));
            while kept.len() > KEPT {
                kept.pop_front();
            }
        }
    }

    /// Somebody looks again: the frame that came last while nobody did is theirs to see.
    fn seen(&self) {
        let unseen = (self.unseen.lock())
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(Kept(buffer)) = unseen else { return };
        // SAFETY: a live pixel buffer, whose surface is lent for as long as it's held.
        let surface = unsafe { sys::CVPixelBufferGetIOSurface(ns::raw(&buffer)) };
        if !surface.is_null() {
            *self.sent.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
            self.pass(surface, Some(buffer));
        }
    }
}

/// Asks for a window alone, without the windows on it (a sheet). With them a stream's
/// picture is the two together, whichever of them is asked for and whatever its size, and
/// a screenshot isn't made at all. macOS before 14.2 hasn't the setting.
pub fn alone(configuration: &AnyObject) {
    let setter = objc2::sel!(setIncludeChildWindows:);
    // SAFETY: any object answers `respondsToSelector:`; the setter takes a Boolean.
    unsafe {
        let has: bool = msg_send![configuration, respondsToSelector: setter];
        if has {
            let _: () = msg_send![configuration, setIncludeChildWindows: false];
        }
    }
}

/// Frames a stream keeps in hand: `ScreenCaptureKit` wants three at least.
const DEPTH: isize = 6;
const BACKDROP_DEPTH: isize = 5;

/// How a window of `size` points is captured: whole, alone, at `scale` pixels a point, at
/// the rate for a desktop that is watched or not, `depth` frames kept in hand.
fn configuration(scale: f64, size: (f64, f64), watched: bool, depth: isize) -> Obj {
    let pixels = |points: f64| ((points * scale).round() as usize) & !1;
    let interval = Time {
        value: 1,
        timescale: if watched { RATE } else { UNWATCHED_RATE },
        flags: 1,
        epoch: 0,
    };
    // SAFETY: `ScreenCaptureKit`'s own constructor and setters.
    unsafe {
        let configuration: Obj = msg_send![class!(SCStreamConfiguration), new];
        let _: () = msg_send![&*configuration, setWidth: pixels(size.0)];
        let _: () = msg_send![&*configuration, setHeight: pixels(size.1)];
        alone(&configuration);
        let _: () = msg_send![&*configuration, setMinimumFrameInterval: interval];
        let _: () = msg_send![&*configuration, setPixelFormat: BGRA];
        srgb(&configuration);
        let _: () = msg_send![&*configuration, setQueueDepth: depth];
        let _: () = msg_send![&*configuration, setShowsCursor: false];
        configuration
    }
}

/// Asks for pixels in sRGB, which is what the app draws them as. Left alone they come in
/// the display's own colours (Display P3 on a laptop's own screen), and shown as sRGB those
/// look washed out.
pub fn srgb(configuration: &AnyObject) {
    let name = ns::string("kCGColorSpaceSRGB");
    // SAFETY: the setter takes the colour space's name, a Core Foundation string, which an
    // `NSString` is.
    unsafe {
        let name: *const sys::Text = ns::raw(&name).cast();
        let _: () = msg_send![configuration, setColorSpaceName: name];
    }
}

struct Stream {
    capture: Obj,
    /// Kept as long as the stream sends to it.
    output: Retained<Output>,
    /// The size it was started at, in points: a window that changes size starts afresh.
    size: (f64, f64),
    /// The size the window has now, and since when.
    wanted: ((f64, f64), Instant),
    /// The desktop's own picture, which keeps the slow rate whoever looks.
    backdrop: bool,
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: a stream answers `stopCaptureWithCompletionHandler:`, whose handler may be nil.
        unsafe {
            let _: () = msg_send![
                &*self.capture,
                stopCaptureWithCompletionHandler: ns::no_handler::<dyn Fn(*mut AnyObject)>()
            ];
        }
    }
}

/// The streams of the desktop's windows.
pub struct Streams {
    sender: Arc<mach::Sender>,
    /// The pixels a point of the display the desktop is on.
    pub scale: f64,
    running: HashMap<u32, Stream>,
    /// What can be captured, as `ScreenCaptureKit` last listed it.
    listed: Slot,
    /// The windows a listing that hasn't answered yet was asked for, and when it was asked.
    wanted: Option<Vec<u32>>,
    asked: Option<Instant>,
    /// The windows the last listing was asked for and didn't have: one that shares nothing.
    /// They're asked for again after a while, not at once.
    lacked: Vec<u32>,
    /// Frames sent since the host started, and those among them in which more changed than
    /// a text cursor.
    pub frames: Arc<AtomicU64>,
    pub delivered: Arc<AtomicU64>,
    pub changes: Arc<AtomicU64>,
    watched: Arc<AtomicBool>,
}

impl Streams {
    /// Streams for windows of a desktop still to be made: its display's scale is set as
    /// it is.
    pub fn new(sender: mach::Sender) -> Streams {
        Streams {
            sender: Arc::new(sender),
            scale: 1.0,
            running: HashMap::new(),
            listed: Slot::default(),
            wanted: None,
            asked: None,
            lacked: Vec::new(),
            frames: Arc::new(AtomicU64::new(0)),
            delivered: Arc::new(AtomicU64::new(0)),
            changes: Arc::new(AtomicU64::new(0)),
            watched: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Makes the streams match `windows` and the desktop's own picture, one for each, at the
    /// rate for a desktop that is `watched` (a tab shows it, a wait is on) or not.
    pub fn sync(&mut self, windows: &[Window], backdrop: Option<&Window>, watched: bool) {
        let was = self.watched.swap(watched, Ordering::Relaxed);
        if watched && !was {
            for stream in self.running.values() {
                stream.output.ivars().seen();
            }
        }
        let all = || windows.iter().chain(backdrop);
        self.running.retain(|id, stream| {
            let Some(window) = all().find(|window| window.id == *id) else {
                return false;
            };
            let size = (window.frame.width, window.frame.height);
            if size != stream.wanted.0 {
                stream.wanted = (size, Instant::now());
            }
            // A window being resized keeps its stream, whose frames the app stretches to
            // it, until its size has stood a moment: then one of that size takes over.
            size == stream.size || stream.wanted.1.elapsed() < RESIZED_FOR
        });
        let scale = self.scale;
        let paced = |stream: &&mut Stream| !stream.backdrop && was != watched;
        for stream in self.running.values_mut().filter(paced) {
            let configuration = configuration(scale, stream.size, watched, DEPTH);
            // SAFETY: a stream takes a new configuration while it runs; the handler may be nil.
            unsafe {
                let _: () = msg_send![
                    &*stream.capture,
                    updateConfiguration: &*configuration,
                    completionHandler: ns::no_handler::<dyn Fn(*mut AnyObject)>()
                ];
            }
        }
        if let Some(content) = shot::take(&self.listed) {
            let shareable = shot::shareable(&content);
            for window in all() {
                if self.running.contains_key(&window.id) {
                    continue;
                }
                if let Some(found) = shot::numbered(&shareable, window.id) {
                    let own = backdrop.is_some_and(|backdrop| backdrop.id == window.id);
                    let stream = self.start(window, own, watched, found);
                    self.running.insert(window.id, stream);
                }
            }
            let wanted = self.wanted.take().unwrap_or_default();
            self.lacked = (wanted.into_iter())
                .filter(|id| !self.running.contains_key(id))
                .collect();
        }
        let missing: Vec<u32> = (all().map(|window| window.id))
            .filter(|id| !self.running.contains_key(id))
            .collect();
        // A listing that failed never answers, and one may lack a window that shares
        // nothing: both are asked for again after a while. A window that is new is asked
        // for at once.
        let again = (self.asked).is_none_or(|asked| asked.elapsed() > ASKS_AGAIN_AFTER);
        if again {
            self.wanted = None;
        }
        let news = missing.iter().any(|id| !self.lacked.contains(id));
        if !missing.is_empty() && self.wanted.is_none() && (news || again) {
            self.wanted = Some(missing);
            self.asked = Some(Instant::now());
            self.ask();
        }
    }

    /// Asks `ScreenCaptureKit` what can be captured; the answer comes to `listed`.
    fn ask(&self) {
        let done = shot::into(&self.listed);
        // SAFETY: the class's own listing call, whose block the callee copies.
        unsafe {
            let _: () = msg_send![
                class!(SCShareableContent),
                getShareableContentExcludingDesktopWindows: false,
                onScreenWindowsOnly: false,
                completionHandler: &*done
            ];
        }
    }

    fn start(&self, window: &Window, backdrop: bool, watched: bool, shareable: &Obj) -> Stream {
        let size = (window.frame.width, window.frame.height);
        // The desktop's own picture (its wallpaper) is as large as the display and seldom
        // changes: a pixel a point, slowly, and few frames in hand, whoever looks.
        let (scale, watched, depth) = if backdrop {
            (1.0, false, BACKDROP_DEPTH)
        } else {
            (self.scale, watched, DEPTH)
        };
        let output = Output::alloc().set_ivars(Sink {
            window: window.id,
            sender: Arc::clone(&self.sender),
            sequence: AtomicU32::new(0),
            frames: Arc::clone(&self.frames),
            delivered: Arc::clone(&self.delivered),
            changes: Arc::clone(&self.changes),
            watched: Arc::clone(&self.watched),
            counts: !backdrop,
            scale,
            last: Mutex::default(),
            sent: Mutex::default(),
            kept: Mutex::new(VecDeque::new()),
            unseen: Mutex::default(),
        });
        // SAFETY: `NSObject`'s `init` on a fresh instance with its ivars set; then
        // `ScreenCaptureKit`'s own constructors and setters, a queue made for the stream, and
        // completion handlers that may be nil.
        unsafe {
            let output: Retained<Output> = msg_send![super(output), init];
            let filter: Allocated<AnyObject> = msg_send![class!(SCContentFilter), alloc];
            let filter: Obj = msg_send![filter, initWithDesktopIndependentWindow: &**shareable];
            let configuration = configuration(scale, size, watched, depth);
            let stream: Allocated<AnyObject> = msg_send![class!(SCStream), alloc];
            let stream: Obj = msg_send![
                stream,
                initWithFilter: &*filter,
                configuration: &*configuration,
                delegate: std::ptr::null::<AnyObject>()
            ];
            let queue =
                sys::dispatch_queue_create(c"dev.koyo.desk.capture".as_ptr(), std::ptr::null());
            let _: bool = msg_send![
                &*stream,
                addStreamOutput: &*output,
                type: 0isize,
                sampleHandlerQueue: queue.cast::<AnyObject>(),
                error: std::ptr::null_mut::<*mut AnyObject>()
            ];
            let _: () = msg_send![
                &*stream,
                startCaptureWithCompletionHandler: ns::no_handler::<dyn Fn(*mut AnyObject)>()
            ];
            Stream {
                capture: stream,
                output,
                size,
                wanted: (size, Instant::now()),
                backdrop,
            }
        }
    }
}
