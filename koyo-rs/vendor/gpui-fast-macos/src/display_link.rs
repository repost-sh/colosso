//! Frame pacing for macOS windows.
//!
//! On macOS 14 and later a window is paced by a `CAMetalDisplayLink` on its
//! layer ([`gpui_apple::display_link::MetalFrameSource`]). Earlier systems use
//! the shared `CVDisplayLink` registry described below ([`WindowFrameSource`]).
//!
//! CVDisplayLink has no safe teardown: `CVDisplayLinkStop` merely flags the
//! link's io thread to exit "soon" and returns immediately, and there is no
//! way to learn when (or whether) a final output callback has finished. Two
//! crash classes came from ignoring this:
//!
//! * releasing the link after `stop` let the still-running io thread read the
//!   freed link's internals (segfault in `CVHWTime::reset`, fixed by leaking
//!   the link in #32116);
//! * releasing the dispatch source that the output callback dereferenced via
//!   its context pointer let a straggler callback read freed memory (segfault
//!   in `dispatch_source_merge_data`, Sentry issue ZED-7XR).
//!
//! Instead of leaking one link and one source per teardown (and teardown
//! happens on every resize tick, activation, and occlusion change), we keep a
//! single immortal `CVDisplayLink` per display in a static registry and have
//! windows subscribe to it:
//!
//! * Links are created lazily and never released, so the release race cannot
//!   occur. The output callback's context is the display id (an integer, not
//!   a pointer), and the only memory the callback dereferences is the static
//!   registry, so a straggler callback after `stop` is a harmless no-op.
//! * Each window owns one dispatch source for the life of the window. On
//!   window close it is removed from the registry under the lock (making it
//!   unreachable from the callback), cancelled, and genuinely released.
//! * A display's link runs iff it has subscribers; windows never start or
//!   stop links directly, so interleaved starts/stops of windows sharing a
//!   display cannot conflict.
//!
//! One tradeoff of immortal entries: a link created for a given
//! `CGDirectDisplayID` is reused forever, including after the display is
//! unplugged and one reappears with the same id, or after mode/refresh-rate
//! changes. `CVDisplayLink` looks up display timing dynamically, so a cached
//! link keeps pacing correctly; if that ever proves untrue the fix is to
//! also refresh entries from a display-reconfiguration callback, not to
//! release links (which would reintroduce the teardown race).
//!
//! Lock ordering: the output callback runs on the link's io thread and takes
//! the registry lock, possibly while holding CVDisplayLink-internal locks. To
//! avoid a lock cycle through those (undocumented) internals, we never call a
//! CoreVideo function while holding the registry lock. Registry mutations and
//! link start/stop happen on the main thread only, which keeps the `running`
//! flag and the link's actual state consistent without holding the lock
//! across the calls.
//!
//! `std::sync::Mutex` rather than `parking_lot` is deliberate: on macOS it is
//! currently backed by `os_unfair_lock`, whose priority donation resolves
//! inversions between the high-priority io thread and the main thread. (That
//! is a std implementation detail, not a guarantee; if it changes, the cost
//! is added latency under contention, not incorrectness.)

use crate::main_thread_marker;
use anyhow::Result;
use collections::FxHashMap;
use core_graphics::display::CGDirectDisplayID;
use dispatch2::{
    _dispatch_source_type_data_add, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
};
use gpui::{CompositionSurfaceId, FrameRequestSource, PlatformFrameSignal};
use gpui_apple::display_link::MetalFrameSource;
use gpui_util::ResultExt;
use scheduler::Instant;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, btree_map},
    ffi::c_void,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

struct Registry {
    displays: BTreeMap<CGDirectDisplayID, DisplayEntry>,
    next_subscriber_id: u64,
}

impl Registry {
    const fn new() -> Self {
        Registry {
            displays: BTreeMap::new(),
            next_subscriber_id: 0,
        }
    }
}

struct DisplayEntry {
    link: sys::DisplayLink,
    running: bool,
    subscribers: Vec<DisplayLinkSubscriber>,
}

struct DisplayLinkSubscriber {
    id: SubscriberId,
    frame_requests: DispatchRetained<DispatchSource>,
    signal: Arc<PlatformFrameSignal>,
}

// SAFETY: Both fields wrapping raw pointers are refcounted handles to
// thread-safe objects that are valid on any thread: `sys::DisplayLink` to a
// CoreVideo object, and each subscriber's `DispatchRetained<DispatchSource>`
// to a GCD object (which the display's io thread really does use, calling
// `merge_data` from the output callback). All mutation of the entry itself
// is serialized by the registry lock.
unsafe impl Send for DisplayEntry {}

#[derive(Copy, Clone, PartialEq, Eq)]
struct SubscriberId(u64);

fn lock_registry() -> MutexGuard<'static, Registry> {
    // Proceeding past poison is safe here (the map's invariants hold after
    // any partial mutation), and panicking instead would abort the process
    // when this is reached from the extern "C" output callback.
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

fn debug_assert_main_thread() {
    debug_assert!(
        main_thread_marker().is_some(),
        "display link registry mutations must happen on the main thread; \
         the registry's lock ordering and state consistency depend on it"
    );
}

unsafe extern "C" fn display_link_output_callback(
    _display_link_out: *mut sys::CVDisplayLink,
    current_time: *const sys::CVTimeStamp,
    _output_time: *const sys::CVTimeStamp,
    _flags_in: i64,
    _flags_out: *mut i64,
    display_id: *mut c_void,
) -> i32 {
    let signal_at = PlatformFrameSignal::capture(|| {
        // Instant cannot be constructed from Mach ticks, so sample both clock domains.
        let current_host_time = objc2_core_video::CVGetCurrentHostTime();
        let received_at = Instant::now();
        let host_clock_frequency = objc2_core_video::CVGetHostClockFrequency();
        // SAFETY: CoreVideo owns this timestamp for the lifetime of the callback.
        let current_time = unsafe { current_time.as_ref() };
        frame_request_instant(
            current_time,
            current_host_time,
            host_clock_frequency,
            received_at,
        )
    });
    let display_id = display_id as usize as CGDirectDisplayID;
    let registry = lock_registry();
    if let Some(entry) = registry.displays.get(&display_id) {
        for subscriber in &entry.subscribers {
            if let Some(signal_at) = signal_at {
                subscriber
                    .signal
                    .record(signal_at, FrameRequestSource::NativeCallback);
            }
            subscriber.frame_requests.merge_data(1);
        }
    }
    0
}

/// Maps a CoreVideo host timestamp using a paired clock sample.
///
/// Invalid timestamps or clock conversions fall back to callback receipt time.
fn frame_request_instant(
    timestamp: Option<&sys::CVTimeStamp>,
    current_host_time: u64,
    host_clock_frequency: f64,
    received_at: Instant,
) -> Instant {
    let Some(timestamp) = timestamp.filter(|timestamp| {
        timestamp.version == 0 && timestamp.flags & sys::kCVTimeStampHostTimeValid != 0
    }) else {
        return received_at;
    };
    if !host_clock_frequency.is_finite() || host_clock_frequency <= 0.0 {
        return received_at;
    }
    current_host_time
        .checked_sub(timestamp.host_time)
        .and_then(|elapsed_ticks| {
            Duration::try_from_secs_f64(elapsed_ticks as f64 / host_clock_frequency).ok()
        })
        .and_then(|elapsed| received_at.checked_sub(elapsed))
        .unwrap_or(received_at)
}

fn subscribe(
    display_id: CGDirectDisplayID,
    frame_requests: DispatchRetained<DispatchSource>,
    signal: Arc<PlatformFrameSignal>,
) -> Result<SubscriberId> {
    debug_assert_main_thread();

    let needs_link = !lock_registry().displays.contains_key(&display_id);
    let new_link = if needs_link {
        // Created outside the registry lock; see the lock ordering note above.
        Some(unsafe {
            sys::DisplayLink::new(
                display_id,
                display_link_output_callback,
                display_id as usize as *mut c_void,
            )?
        })
    } else {
        None
    };

    let (subscriber_id, link_to_start) = {
        let mut registry = lock_registry();
        let registry = &mut *registry;
        let subscriber_id = SubscriberId(registry.next_subscriber_id);
        registry.next_subscriber_id += 1;
        let entry = match (registry.displays.entry(display_id), new_link) {
            // If an entry appeared since the check above, dropping `new_link`
            // is safe: a never-started link has no io thread, so releasing it
            // doesn't race.
            (btree_map::Entry::Occupied(entry), _) => entry.into_mut(),
            (btree_map::Entry::Vacant(vacant), Some(link)) => vacant.insert(DisplayEntry {
                link,
                running: false,
                subscribers: Vec::new(),
            }),
            (btree_map::Entry::Vacant(_), None) => {
                // Entries are never removed, so an entry observed above cannot
                // have disappeared while subscriptions stay on the main thread.
                anyhow::bail!("display link registry entry vanished for display {display_id}");
            }
        };
        entry.subscribers.push(DisplayLinkSubscriber {
            id: subscriber_id,
            frame_requests,
            signal,
        });
        let link_to_start = if entry.running {
            None
        } else {
            entry.running = true;
            // Clone the refcounted handle so the CVDisplayLinkStart call can
            // happen after the lock is released.
            Some(entry.link.clone())
        };
        (subscriber_id, link_to_start)
    };

    if let Some(mut link) = link_to_start {
        if let Err(error) = unsafe { link.start() } {
            let mut registry = lock_registry();
            if let Some(entry) = registry.displays.get_mut(&display_id) {
                entry.running = false;
                entry.subscribers.retain(|subscriber| {
                    if subscriber.id == subscriber_id {
                        subscriber.signal.take();
                        false
                    } else {
                        true
                    }
                });
            }
            return Err(error);
        }
    }

    Ok(subscriber_id)
}

fn unsubscribe(display_id: CGDirectDisplayID, subscriber_id: SubscriberId) {
    debug_assert_main_thread();

    let link_to_stop = {
        let mut registry = lock_registry();
        let Some(entry) = registry.displays.get_mut(&display_id) else {
            return;
        };
        entry
            .subscribers
            .retain(|subscriber| subscriber.id != subscriber_id);
        if entry.subscribers.is_empty() && entry.running {
            entry.running = false;
            Some(entry.link.clone())
        } else {
            None
        }
    };

    if let Some(mut link) = link_to_stop {
        // A final output callback can still fire after this returns; it finds
        // no subscribers for this display and does nothing.
        unsafe { link.stop().log_err() };
    }
}

/// Koyo: what [`MetalFrames`] asks of a display link, so that a test, which has no main
/// thread to make a `CAMetalDisplayLink` on, can stand links of its own in.
pub trait FrameLink {
    fn set_paused(&self, paused: bool);
    fn is_paused(&self) -> bool;
}

// Koyo: a window's links are `CAMetalDisplayLink`s.
impl FrameLink for MetalFrameSource {
    fn set_paused(&self, paused: bool) {
        MetalFrameSource::set_paused(self, paused);
    }

    fn is_paused(&self) -> bool {
        MetalFrameSource::is_paused(self)
    }
}

/// Koyo: a link a test makes off the main thread. The test keeps its [`TestLinkState`], which
/// says whether the link is paused and whether it's been dropped, as a `CAMetalDisplayLink`
/// is invalidated when it is.
#[cfg(test)]
pub(crate) struct TestLink(std::rc::Rc<TestLinkState>);

#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestLinkState {
    paused: Cell<bool>,
    dropped: Cell<bool>,
}

#[cfg(test)]
impl TestLink {
    pub(crate) fn new() -> (Self, std::rc::Rc<TestLinkState>) {
        let state = std::rc::Rc::<TestLinkState>::default();
        (Self(state.clone()), state)
    }
}

#[cfg(test)]
impl TestLinkState {
    pub(crate) fn is_paused(&self) -> bool {
        self.paused.get()
    }

    pub(crate) fn is_live(&self) -> bool {
        !self.dropped.get()
    }
}

#[cfg(test)]
impl Drop for TestLink {
    fn drop(&mut self) {
        self.0.dropped.set(true);
    }
}

#[cfg(test)]
impl FrameLink for TestLink {
    fn set_paused(&self, paused: bool) {
        self.0.paused.set(paused);
    }

    fn is_paused(&self) -> bool {
        self.0.paused.get()
    }
}

/// A window's `CAMetalDisplayLink`, shared with GPUI's frame waker so that a
/// wake reaches the current link without taking the window's lock.
///
/// The link pauses after any update in which GPUI neither drew, presented,
/// nor asked for another frame, so an idle window acquires no drawables. GPUI
/// decides when frames are needed (including presenting after high-rate
/// input); this only stops the link between those decisions.
///
/// Koyo: a window that composes native views has a layer over them for each of its other
/// GPUI surfaces, and each of those layers has a link of its own here ([`WindowLinks`]),
/// which hands it its drawables as the window's hands the window's layer its own. Upstream
/// gave only the window's layer a link, and a layer over a native view asked Core Animation
/// for a drawable in the frame, on the main thread, which waits for as long as Core
/// Animation keeps the layer's: a second a frame after the display woke.
pub struct MetalFrames<Link = MetalFrameSource> {
    // Koyo: the window's links, its own layer's and its other layers', there together or not
    // at all. Upstream held the window's own alone.
    links: RefCell<Option<WindowLinks<Link>>>,
    /// Whether GPUI asked for a frame since the current update began.
    woken: Cell<bool>,
}

/// Koyo: the links of a window while they're live: its own layer's, and one for each layer
/// over a native view, by the layer's surface. They run and pause together, so a layer's
/// link is made paused or running as the window's own is then ([`MetalFrames::link_layer`]);
/// no layer has a link while the window's own has none.
struct WindowLinks<Link> {
    window: Link,
    layers: FxHashMap<CompositionSurfaceId, Link>,
}

impl<Link: FrameLink> WindowLinks<Link> {
    fn set_paused(&self, paused: bool) {
        self.window.set_paused(paused);
        for link in self.layers.values() {
            link.set_paused(paused);
        }
    }
}

// Koyo: `Default` by hand, as a derived one would ask it of `Link`.
impl<Link> Default for MetalFrames<Link> {
    fn default() -> Self {
        Self {
            links: RefCell::new(None),
            woken: Cell::new(false),
        }
    }
}

impl<Link: FrameLink> MetalFrames<Link> {
    /// Koyo: the window's own link, made anew, starts with no layer linked; `None` drops the
    /// links of every layer with the window's own.
    pub fn replace(&self, source: Option<Link>) {
        let links = source.map(|window| WindowLinks {
            window,
            layers: FxHashMap::default(),
        });
        let replaced = self.links.replace(links);
        drop(replaced);
    }

    pub fn is_attached(&self) -> bool {
        self.links.borrow().is_some()
    }

    /// Koyo: gives the layer of `surface` the link `make` makes, paused or running as the
    /// window's own is, while the window has its own. Returns whether the layer has its link:
    /// a window whose links don't run makes none for its layers, which get theirs with the
    /// window's own.
    pub fn link_layer(
        &self,
        surface: CompositionSurfaceId,
        make: impl FnOnce() -> Option<Link>,
    ) -> bool {
        let mut links = self.links.borrow_mut();
        let Some(links) = links.as_mut() else {
            return false;
        };
        let Some(link) = make() else {
            return false;
        };
        link.set_paused(links.window.is_paused());
        drop(links.layers.insert(surface, link));
        true
    }

    /// Koyo: takes the link of the layer of `surface` away, as the layer leaves the window.
    pub fn unlink_layer(&self, surface: CompositionSurfaceId) {
        let unlinked = self
            .links
            .borrow_mut()
            .as_mut()
            .and_then(|links| links.layers.remove(&surface));
        drop(unlinked);
    }

    pub fn wake(&self) {
        self.woken.set(true);
        // A source being replaced starts unpaused, so skipping it is safe.
        // Koyo: every link of the window starts.
        if let Ok(links) = self.links.try_borrow()
            && let Some(links) = links.as_ref()
        {
            links.set_paused(false);
        }
    }

    pub fn begin_frame(&self) {
        self.woken.set(false);
    }

    pub fn debug_state(&self) -> serde_json::Value {
        let links = self.links.try_borrow().ok();
        let links = links.as_ref().and_then(|links| links.as_ref());
        serde_json::json!({
            "kind": "CAMetalDisplayLink",
            "attached": self.is_attached(),
            "paused": links.map(|links| links.window.is_paused()),
            // Koyo: each layer's over a native view.
            "layers_paused": links.map(|links| {
                links.layers.values().map(Link::is_paused).collect::<Vec<_>>()
            }),
            "woken": self.woken.get(),
        })
    }

    /// Ends an update, pausing the link if GPUI neither used its drawable nor
    /// asked for another frame while it ran. Returns whether it paused.
    ///
    /// Koyo: the layers' links pause with the window's, and none pauses while a layer still
    /// waits for a drawable (`awaiting_drawable`): its link is what hands it one.
    pub fn end_frame(&self, used_drawable: bool, awaiting_drawable: bool) -> bool {
        let pause = !used_drawable && !awaiting_drawable && !self.woken.get();
        if pause && let Some(links) = self.links.borrow().as_ref() {
            links.set_paused(true);
        }
        pause
    }
}

/// A per-window source of frame requests, paced by the display the window is
/// on. The wrapped dispatch source coalesces vsync ticks from the display's
/// io thread and invokes `callback(data)` on the main queue.
pub struct WindowFrameSource {
    frame_requests: DispatchRetained<DispatchSource>,
    registration: Option<(CGDirectDisplayID, SubscriberId)>,
    signal: Arc<PlatformFrameSignal>,
}

impl WindowFrameSource {
    pub fn new(data: *mut c_void, callback: extern "C" fn(*mut c_void)) -> Self {
        let frame_requests = unsafe {
            let frame_requests = DispatchSource::new(
                &raw const _dispatch_source_type_data_add as *mut _,
                0,
                0,
                Some(DispatchQueue::main()),
            );
            frame_requests.set_context(data);
            frame_requests.set_event_handler_f(callback);
            // Resume before this source can ever be dropped: destroying a
            // suspended dispatch source is undefined behavior (#50875).
            frame_requests.resume();
            frame_requests
        };
        Self {
            frame_requests,
            registration: None,
            signal: Arc::new(PlatformFrameSignal::new()),
        }
    }

    pub fn start(&mut self, display_id: CGDirectDisplayID) -> Result<()> {
        self.stop();
        let subscriber_id =
            subscribe(display_id, self.frame_requests.clone(), self.signal.clone())?;
        self.registration = Some((display_id, subscriber_id));
        Ok(())
    }

    pub fn debug_state(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "CVDisplayLink",
            "attached": self.registration.is_some(),
            "display_id": self.registration.map(|(display_id, _)| display_id),
        })
    }

    pub fn take_signal(&self) -> Option<(Instant, FrameRequestSource)> {
        self.signal.take()
    }

    pub fn stop(&mut self) {
        if let Some((display_id, subscriber_id)) = self.registration.take() {
            unsubscribe(display_id, subscriber_id);
        }
        self.signal.take();
    }
}

impl Drop for WindowFrameSource {
    fn drop(&mut self) {
        self.stop();
        // Unsubscribing makes this source unreachable from the output
        // callback, so unlike before (ZED-7XR) it is safe to actually release
        // it. Cancelling first guarantees the event handler never runs again;
        // its context points at the window's native view, which may be
        // deallocated after this.
        self.frame_requests.cancel();
    }
}

mod sys {
    //! Derived from display-link crate under the following license:
    //! <https://github.com/BrainiumLLC/display-link/blob/master/LICENSE-MIT>
    //! Apple docs: [CVDisplayLink](https://developer.apple.com/documentation/corevideo/cvdisplaylinkoutputcallback?language=objc)
    #![allow(dead_code, non_upper_case_globals)]

    use anyhow::Result;
    use core_graphics::display::CGDirectDisplayID;
    use foreign_types::{ForeignType, foreign_type};
    use std::{
        ffi::c_void,
        fmt::{self, Debug, Formatter},
    };

    #[derive(Debug)]
    pub enum CVDisplayLink {}

    foreign_type! {
        pub unsafe type DisplayLink {
            type CType = CVDisplayLink;
            fn drop = CVDisplayLinkRelease;
            fn clone = CVDisplayLinkRetain;
        }
    }

    impl Debug for DisplayLink {
        fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
            formatter
                .debug_tuple("DisplayLink")
                .field(&self.as_ptr())
                .finish()
        }
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(crate) struct CVTimeStamp {
        pub version: u32,
        pub video_time_scale: i32,
        pub video_time: i64,
        pub host_time: u64,
        pub rate_scalar: f64,
        pub video_refresh_period: i64,
        pub smpte_time: CVSMPTETime,
        pub flags: u64,
        pub reserved: u64,
    }

    pub type CVTimeStampFlags = u64;

    pub const kCVTimeStampVideoTimeValid: CVTimeStampFlags = 1 << 0;
    pub const kCVTimeStampHostTimeValid: CVTimeStampFlags = 1 << 1;
    pub const kCVTimeStampSMPTETimeValid: CVTimeStampFlags = 1 << 2;
    pub const kCVTimeStampVideoRefreshPeriodValid: CVTimeStampFlags = 1 << 3;
    pub const kCVTimeStampRateScalarValid: CVTimeStampFlags = 1 << 4;
    pub const kCVTimeStampTopField: CVTimeStampFlags = 1 << 16;
    pub const kCVTimeStampBottomField: CVTimeStampFlags = 1 << 17;
    pub const kCVTimeStampVideoHostTimeValid: CVTimeStampFlags =
        kCVTimeStampVideoTimeValid | kCVTimeStampHostTimeValid;
    pub const kCVTimeStampIsInterlaced: CVTimeStampFlags =
        kCVTimeStampTopField | kCVTimeStampBottomField;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub(crate) struct CVSMPTETime {
        pub subframes: i16,
        pub subframe_divisor: i16,
        pub counter: u32,
        pub time_type: u32,
        pub flags: u32,
        pub hours: i16,
        pub minutes: i16,
        pub seconds: i16,
        pub frames: i16,
    }

    pub type CVSMPTETimeType = u32;

    pub const kCVSMPTETimeType24: CVSMPTETimeType = 0;
    pub const kCVSMPTETimeType25: CVSMPTETimeType = 1;
    pub const kCVSMPTETimeType30Drop: CVSMPTETimeType = 2;
    pub const kCVSMPTETimeType30: CVSMPTETimeType = 3;
    pub const kCVSMPTETimeType2997: CVSMPTETimeType = 4;
    pub const kCVSMPTETimeType2997Drop: CVSMPTETimeType = 5;
    pub const kCVSMPTETimeType60: CVSMPTETimeType = 6;
    pub const kCVSMPTETimeType5994: CVSMPTETimeType = 7;

    pub type CVSMPTETimeFlags = u32;

    pub const kCVSMPTETimeValid: CVSMPTETimeFlags = 1 << 0;
    pub const kCVSMPTETimeRunning: CVSMPTETimeFlags = 1 << 1;

    pub type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
        display_link_out: *mut CVDisplayLink,
        // A pointer to the current timestamp. This represents the timestamp when the callback is called.
        current_time: *const CVTimeStamp,
        // A pointer to the output timestamp. This represents the timestamp for when the frame will be displayed.
        output_time: *const CVTimeStamp,
        // Unused
        flags_in: i64,
        // Unused
        flags_out: *mut i64,
        // A pointer to app-defined data.
        display_link_context: *mut c_void,
    ) -> i32;

    #[link(name = "CoreFoundation", kind = "framework")]
    #[link(name = "CoreVideo", kind = "framework")]
    #[allow(improper_ctypes, unknown_lints, clippy::duplicated_attributes)]
    unsafe extern "C" {
        pub fn CVDisplayLinkCreateWithActiveCGDisplays(
            display_link_out: *mut *mut CVDisplayLink,
        ) -> i32;
        pub fn CVDisplayLinkSetCurrentCGDisplay(
            display_link: &mut DisplayLinkRef,
            display_id: u32,
        ) -> i32;
        pub fn CVDisplayLinkSetOutputCallback(
            display_link: &mut DisplayLinkRef,
            callback: CVDisplayLinkOutputCallback,
            user_info: *mut c_void,
        ) -> i32;
        pub fn CVDisplayLinkStart(display_link: &mut DisplayLinkRef) -> i32;
        pub fn CVDisplayLinkStop(display_link: &mut DisplayLinkRef) -> i32;
        pub fn CVDisplayLinkRelease(display_link: *mut CVDisplayLink);
        pub fn CVDisplayLinkRetain(display_link: *mut CVDisplayLink) -> *mut CVDisplayLink;
    }

    impl DisplayLink {
        /// Apple docs: [CVDisplayLinkCreateWithCGDisplay](https://developer.apple.com/documentation/corevideo/1456981-cvdisplaylinkcreatewithcgdisplay?language=objc)
        pub unsafe fn new(
            display_id: CGDirectDisplayID,
            callback: CVDisplayLinkOutputCallback,
            user_info: *mut c_void,
        ) -> Result<Self> {
            unsafe {
                let mut display_link: *mut CVDisplayLink = 0 as _;

                let code = CVDisplayLinkCreateWithActiveCGDisplays(&mut display_link);
                anyhow::ensure!(code == 0, "could not create display link, code: {}", code);

                let mut display_link = DisplayLink::from_ptr(display_link);

                let code = CVDisplayLinkSetOutputCallback(&mut display_link, callback, user_info);
                anyhow::ensure!(code == 0, "could not set output callback, code: {}", code);

                let code = CVDisplayLinkSetCurrentCGDisplay(&mut display_link, display_id);
                anyhow::ensure!(
                    code == 0,
                    "could not assign display to display link, code: {}",
                    code
                );

                Ok(display_link)
            }
        }
    }

    impl DisplayLinkRef {
        /// Apple docs: [CVDisplayLinkStart](https://developer.apple.com/documentation/corevideo/1457193-cvdisplaylinkstart?language=objc)
        pub unsafe fn start(&mut self) -> Result<()> {
            unsafe {
                let code = CVDisplayLinkStart(self);
                anyhow::ensure!(code == 0, "could not start display link, code: {}", code);
                Ok(())
            }
        }

        /// Apple docs: [CVDisplayLinkStop](https://developer.apple.com/documentation/corevideo/1457281-cvdisplaylinkstop?language=objc)
        pub unsafe fn stop(&mut self) -> Result<()> {
            unsafe {
                let code = CVDisplayLinkStop(self);
                anyhow::ensure!(code == 0, "could not stop display link, code: {}", code);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AnyThread;
    use objc2_quartz_core::CAMetalDisplayLink;
    use std::rc::Rc;

    #[test]
    fn metal_frames_pause_only_when_gpui_neither_draws_nor_asks_again() {
        let frames = MetalFrames::<MetalFrameSource>::default();

        frames.begin_frame();
        assert!(
            !frames.end_frame(true, false),
            "a frame that drew keeps the link running"
        );

        frames.begin_frame();
        frames.wake();
        assert!(
            !frames.end_frame(false, false),
            "a wake during the update, like a throttled frame's retry, keeps it running"
        );

        frames.wake();
        frames.begin_frame();
        assert!(
            frames.end_frame(false, false),
            "a wake before the update was served by it"
        );

        frames.begin_frame();
        assert!(
            frames.end_frame(false, false),
            "an idle update pauses the link"
        );
    }

    // Koyo: a window with its own link and one layer linked, and what each link does.
    fn linked_window() -> (MetalFrames<TestLink>, Rc<TestLinkState>, Rc<TestLinkState>) {
        let frames = MetalFrames::<TestLink>::default();
        let ((window, window_seen), (above, above_seen)) = (TestLink::new(), TestLink::new());
        frames.replace(Some(window));
        assert!(frames.link_layer(CompositionSurfaceId::default(), || Some(above)));
        (frames, window_seen, above_seen)
    }

    #[test]
    fn every_link_of_a_window_pauses_and_wakes_with_its_own() {
        let (frames, window, above) = linked_window();
        let paused = || [window.is_paused(), above.is_paused()];

        frames.begin_frame();
        assert!(frames.end_frame(false, false));
        assert_eq!(paused(), [true; 2], "an idle update pauses every link");

        frames.wake();
        assert_eq!(paused(), [false; 2], "a wake starts every link");

        frames.begin_frame();
        assert!(frames.end_frame(false, false));
        frames.unlink_layer(CompositionSurfaceId::default());
        assert!(!above.is_live(), "a layer's link taken away is invalidated");
        frames.wake();
        assert!(!window.is_paused(), "and the window's own wakes alone");
    }

    #[test]
    fn a_layer_linked_later_runs_or_pauses_as_the_window_does_then() {
        for idle in [true, false] {
            let frames = MetalFrames::<TestLink>::default();
            frames.replace(Some(TestLink::new().0));
            frames.begin_frame();
            if !idle {
                frames.wake();
            }
            assert_eq!(frames.end_frame(false, false), idle);
            let (link, seen) = TestLink::new();
            assert!(frames.link_layer(CompositionSurfaceId::default(), || Some(link)));
            assert_eq!(
                seen.is_paused(),
                idle,
                "a layer linked while the window's link is paused={idle} is paused={idle}"
            );
        }
    }

    #[test]
    fn no_layer_has_a_link_while_the_window_has_none() {
        let (frames, window, above) = linked_window();
        frames.replace(None);
        assert!(
            !window.is_live() && !above.is_live(),
            "the window's link goes, and every layer's with it"
        );
        let made = Cell::new(false);
        assert!(!frames.link_layer(CompositionSurfaceId::default(), || {
            made.set(true);
            Some(TestLink::new().0)
        }));
        assert!(!made.get(), "a window without its link makes none for a layer");
    }

    #[test]
    fn links_keep_running_while_a_layer_waits_for_a_drawable() {
        let (frames, window, above) = linked_window();
        let paused = || [window.is_paused(), above.is_paused()];

        frames.begin_frame();
        assert!(
            !frames.end_frame(false, true),
            "a frame that drew nothing, with a layer still waiting, pauses nothing"
        );
        assert_eq!(paused(), [false; 2]);

        frames.begin_frame();
        assert!(frames.end_frame(false, false));
        assert_eq!(paused(), [true; 2], "a layer that has its drawable lets them pause");
    }

    #[test]
    fn a_renderer_draws_normally_after_its_display_link_is_invalidated() {
        if !MetalFrameSource::is_available() {
            return;
        }
        let mut renderer =
            crate::renderer::MetalRenderer::new(Default::default(), false, false).unwrap();
        renderer.update_drawable_size(gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)));
        let layer = renderer.layer().expect("window renderer has a layer");
        // The link is never added to a run loop: attaching it to the layer is
        // the state under test.
        let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), layer);
        renderer.set_display_link_attached(true);
        // Koyo: with no drawable handed out, the renderer draws nothing and asks for none.
        assert_eq!(
            renderer.draw(&gpui::Scene::default()),
            crate::renderer::Presented::AwaitingDrawable
        );

        link.invalidate();
        renderer.set_display_link_attached(false);
        assert_eq!(
            renderer.draw(&gpui::Scene::default()),
            crate::renderer::Presented::Drawn(gpui::size(
                gpui::DevicePixels(64),
                gpui::DevicePixels(64)
            ))
        );
    }

    #[test]
    fn host_timestamp_preserves_request_age_at_different_clock_rates() {
        let received_at = Instant::now();
        for (host_time, current_host_time, frequency, age) in [
            (4_000_000, 10_000_000, 1e9, Duration::from_millis(6)),
            (1_760_000, 2_000_000, 24e6, Duration::from_millis(10)),
        ] {
            let timestamp = timestamp(host_time);
            assert_eq!(
                frame_request_instant(Some(&timestamp), current_host_time, frequency, received_at),
                received_at - age,
            );
        }
    }

    #[test]
    fn invalid_host_timestamps_fall_back_to_receipt_time() {
        let received_at = Instant::now();
        let valid = timestamp(1_000);
        let mut missing_host_time = valid;
        missing_host_time.flags = 0;
        let mut unknown_version = valid;
        unknown_version.version = 1;
        for timestamp in [None, Some(&missing_host_time), Some(&unknown_version)] {
            assert_eq!(
                frame_request_instant(timestamp, 2_000, 1e9, received_at),
                received_at,
            );
        }
        assert_eq!(
            frame_request_instant(Some(&valid), 999, 1e9, received_at),
            received_at,
        );
        for frequency in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE] {
            assert_eq!(
                frame_request_instant(Some(&valid), 2_000, frequency, received_at),
                received_at,
            );
        }
    }

    fn timestamp(host_time: u64) -> sys::CVTimeStamp {
        sys::CVTimeStamp {
            host_time,
            flags: sys::kCVTimeStampHostTimeValid,
            ..Default::default()
        }
    }
}
