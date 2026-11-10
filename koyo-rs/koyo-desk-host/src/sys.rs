//! The C side of macOS the host leans on: accessibility, events, the window list, run loops,
//! and the two private calls its recipe needs. Declared here once; everything else in the
//! crate goes through the safe wrappers built on these.

use std::ffi::{c_char, c_void};

use objc2_core_foundation::{CGPoint, CGRect};

/// Any Core Foundation object, borrowed.
pub type Ref = *const c_void;

/// `AXObserverCallback`.
pub type ObserverCallback =
    unsafe extern "C" fn(observer: Ref, element: Ref, notification: Ref, context: *mut c_void);

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    pub fn AXIsProcessTrusted() -> u8;
    pub fn AXUIElementCopyMultipleAttributeValues(
        element: Ref,
        attributes: Ref,
        options: u32,
        values: *mut Ref,
    ) -> i32;
    pub fn AXUIElementCreateApplication(pid: i32) -> Ref;
    pub fn AXUIElementCreateSystemWide() -> Ref;
    pub fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
    pub fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
    pub fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
    pub fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
    pub fn AXValueCreate(kind: u32, value: *const c_void) -> Ref;
    pub fn AXValueGetValue(value: Ref, kind: u32, out: *mut c_void) -> u8;
    pub fn AXValueGetTypeID() -> usize;
    pub fn AXObserverCreate(pid: i32, callback: ObserverCallback, out: *mut Ref) -> i32;
    pub fn AXObserverAddNotification(
        observer: Ref,
        element: Ref,
        notification: Ref,
        context: *mut c_void,
    ) -> i32;
    pub fn AXObserverGetRunLoopSource(observer: Ref) -> Ref;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGEventSourceCreate(state: i32) -> Ref;
    pub fn CGEventCreateMouseEvent(source: Ref, kind: u32, point: CGPoint, button: u32) -> Ref;
    pub fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    pub fn CGEventCreateScrollWheelEvent2(
        source: Ref,
        units: u32,
        wheels: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> Ref;
    pub fn CGEventSetFlags(event: Ref, flags: u64);
    pub fn CGEventSetIntegerValueField(event: Ref, field: u32, value: i64);
    pub fn CGEventSetLocation(event: Ref, point: CGPoint);
    pub fn CGEventGetLocation(event: Ref) -> CGPoint;
    pub fn CGEventPostToPid(pid: i32, event: Ref);
    pub fn CGEventKeyboardSetUnicodeString(event: Ref, length: usize, text: *const u16);
    pub fn CGPreflightScreenCaptureAccess() -> bool;
    /// How long ago an event of `kind` last came from `state`'s source, in seconds.
    pub fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
    /// Asks the person for Screen Recording, once, and lists the process in System Settings.
    pub fn CGRequestScreenCaptureAccess() -> bool;
    pub fn CGMainDisplayID() -> u32;
    pub fn CGGetActiveDisplayList(most: u32, displays: *mut u32, count: *mut u32) -> i32;
    pub fn CGDisplayBounds(display: u32) -> CGRect;
    pub fn CGDisplayCopyDisplayMode(display: u32) -> Ref;
    pub fn CGDisplayModeGetWidth(mode: Ref) -> usize;
    pub fn CGDisplayModeGetPixelWidth(mode: Ref) -> usize;
    /// The name a display keeps, as the window server lists its desktops under.
    pub fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> Ref;
    /// Every window's number, as plain values (not number objects), for `option`.
    pub fn CGWindowListCreate(option: u32, relative: u32) -> Ref;
    pub fn CGWindowListCreateDescriptionFromArray(windows: Ref) -> Ref;
    pub fn CGRectMakeWithDictionaryRepresentation(dictionary: Ref, rect: *mut CGRect) -> bool;
    pub fn CGColorSpaceCreateDeviceRGB() -> Ref;
    pub fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Ref,
        bitmap_info: u32,
    ) -> Ref;
    pub fn CGContextDrawImage(context: Ref, rect: CGRect, image: Ref);
    pub fn CGContextSetRGBFillColor(context: Ref, red: f64, green: f64, blue: f64, alpha: f64);
    pub fn CGContextFillRect(context: Ref, rect: CGRect);
    pub fn CGBitmapContextCreateImage(context: Ref) -> Ref;
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    pub fn CGImageDestinationCreateWithData(
        data: Ref,
        kind: Ref,
        count: usize,
        options: Ref,
    ) -> Ref;
    pub fn CGImageDestinationAddImage(destination: Ref, image: Ref, properties: Ref);
    pub fn CGImageDestinationFinalize(destination: Ref) -> bool;
}

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    /// A send right to the surface, for another process to look it up by.
    pub fn IOSurfaceCreateMachPort(surface: Ref) -> u32;
    pub fn IOSurfaceLock(surface: Ref, options: u32, seed: *mut u32) -> i32;
    pub fn IOSurfaceUnlock(surface: Ref, options: u32, seed: *mut u32) -> i32;
    pub fn IOSurfaceGetBaseAddress(surface: Ref) -> *const u8;
    pub fn IOSurfaceGetBytesPerRow(surface: Ref) -> usize;
    pub fn IOSurfaceGetWidth(surface: Ref) -> usize;
    pub fn IOSurfaceGetHeight(surface: Ref) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub static kCFRunLoopDefaultMode: Ref;
    pub fn CFRelease(object: Ref);
    pub fn CFGetTypeID(object: Ref) -> usize;
    pub fn CFUUIDCreateString(allocator: Ref, uuid: Ref) -> Ref;
    pub fn CFArrayCreate(
        allocator: Ref,
        values: *const *const c_void,
        count: isize,
        callbacks: *const c_void,
    ) -> Ref;
    pub fn CFArrayGetCount(array: Ref) -> isize;
    pub fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> bool;
    pub fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> *const c_void;
    pub fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
    pub fn CFDataCreateMutable(allocator: Ref, capacity: isize) -> Ref;
    pub fn CFDataGetBytePtr(data: Ref) -> *const u8;
    pub fn CFDataGetLength(data: Ref) -> isize;
    pub fn CFRunLoopGetCurrent() -> Ref;
    pub fn CFRunLoopGetMain() -> Ref;
    pub fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
    pub fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
    pub fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    pub fn CFRunLoopStop(run_loop: Ref);
}

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    pub fn CMSampleBufferGetImageBuffer(sample: Ref) -> Ref;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    pub fn CVPixelBufferGetIOSurface(buffer: Ref) -> Ref;
}

// Loaded for their classes, reached by message.
#[link(name = "ScreenCaptureKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

unsafe extern "C" {
    pub fn dispatch_queue_create(label: *const c_char, attributes: *const c_void) -> *mut c_void;
}

/// A `CGEvent`, as a message that hands one out says its type.
#[repr(C)]
pub struct Event {
    _opaque: [u8; 0],
}

// SAFETY: a pointer to the opaque `struct __CGEvent`, which is what `CGEventRef` is.
unsafe impl objc2::RefEncode for Event {
    const ENCODING_REF: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("__CGEvent", &[]));
}

/// A Core Foundation string, as a message that takes a `CFStringRef` wants it named.
#[repr(C)]
pub struct Text {
    _opaque: [u8; 0],
}

// SAFETY: a pointer to the opaque `struct __CFString`, which is what `CFStringRef` is.
unsafe impl objc2::RefEncode for Text {
    const ENCODING_REF: objc2::Encoding =
        objc2::Encoding::Pointer(&objc2::Encoding::Struct("__CFString", &[]));
}

/// `kCGEventSourceStatePrivate`: events from it carry none of the person's modifier state.
pub const SOURCE_PRIVATE: i32 = -1;

/// `CGEventType`s.
pub mod event {
    pub const LEFT_DOWN: u32 = 1;
    pub const LEFT_UP: u32 = 2;
    pub const RIGHT_DOWN: u32 = 3;
    pub const RIGHT_UP: u32 = 4;
    pub const MOVED: u32 = 5;
    pub const LEFT_DRAGGED: u32 = 6;
    pub const RIGHT_DRAGGED: u32 = 7;
    pub const OTHER_DOWN: u32 = 25;
    pub const OTHER_UP: u32 = 26;
    pub const OTHER_DRAGGED: u32 = 27;
}

/// `CGEventField`s.
pub mod field {
    pub const CLICK_STATE: u32 = 1;
    /// The window `AppKit` routes the event to (`NSEvent.windowNumber`).
    pub const WINDOW_NUMBER: u32 = 51;
    pub const WINDOW_UNDER_POINTER: u32 = 91;
    pub const WINDOW_THAT_CAN_HANDLE: u32 = 92;
    pub const SCROLL_IS_CONTINUOUS: u32 = 88;
    pub const SCROLL_PHASE: u32 = 99;
    pub const SCROLL_MOMENTUM_PHASE: u32 = 123;
}

/// `kAXValueCGPointType` and `kAXValueCGSizeType`.
pub const AX_POINT: u32 = 1;
pub const AX_SIZE: u32 = 2;

/// A symbol of a system library found at run time, for calls no header declares.
pub fn symbol(library: &std::ffi::CStr, name: &std::ffi::CStr) -> Option<*mut c_void> {
    // SAFETY: two zero-terminated strings; a handle that is never closed, as the library is
    // the system's and stays loaded.
    let found = unsafe {
        let handle = libc::dlopen(library.as_ptr(), libc::RTLD_LAZY);
        libc::dlsym(handle, name.as_ptr())
    };
    (!found.is_null()).then_some(found)
}

/// A function of a system library that no header declares, found by name the first time it's
/// wanted and kept: the lookup walks every loaded image, and what is called through these is
/// called many times a second.
pub struct Symbol {
    library: Option<&'static std::ffi::CStr>,
    name: &'static std::ffi::CStr,
    found: std::sync::OnceLock<Option<usize>>,
}

impl Symbol {
    /// A symbol of `library`.
    pub const fn of(library: &'static std::ffi::CStr, name: &'static std::ffi::CStr) -> Symbol {
        Symbol {
            library: Some(library),
            name,
            found: std::sync::OnceLock::new(),
        }
    }

    /// A symbol among everything already loaded.
    pub const fn loaded(name: &'static std::ffi::CStr) -> Symbol {
        Symbol {
            library: None,
            name,
            found: std::sync::OnceLock::new(),
        }
    }

    /// The function; none on a system that hasn't it.
    ///
    /// # Safety
    /// `F` must be the function pointer type of the symbol.
    pub unsafe fn function<F: Copy>(&self) -> Option<F> {
        let found = self.found.get_or_init(|| {
            let found = match self.library {
                Some(library) => symbol(library, self.name),
                None => loaded_symbol(self.name),
            };
            found.map(<*mut c_void>::expose_provenance)
        });
        // SAFETY: the caller's promise.
        found.map(|address| unsafe { function(std::ptr::with_exposed_provenance_mut(address)) })
    }
}

/// A symbol's address as the function it is.
///
/// # Safety
/// `F` must be the function pointer type of the symbol at `address`.
pub unsafe fn function<F: Copy>(address: *mut c_void) -> F {
    debug_assert_eq!(size_of::<F>(), size_of::<*mut c_void>());
    // SAFETY: the caller's promise; a function pointer and a data pointer are one size here.
    unsafe { std::mem::transmute_copy(&address) }
}

/// A symbol among everything already loaded.
pub fn loaded_symbol(name: &std::ffi::CStr) -> Option<*mut c_void> {
    // SAFETY: a zero-terminated string.
    let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!found.is_null()).then_some(found)
}
