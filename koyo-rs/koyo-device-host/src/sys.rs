//! The C the host calls that no crate of the workspace declares: the run loop, dispatch queues,
//! and functions found by name at run time. libxpc's are in `xpc.rs`.

use std::ffi::{CStr, c_char, c_void};

pub type CFRunLoopRef = *mut c_void;
pub type CFStringRef = *const c_void;
/// A dispatch queue, an Objective-C object where it is passed to one.
pub type DispatchQueue = *mut c_void;

unsafe extern "C" {
    pub static kCFRunLoopDefaultMode: CFStringRef;
    pub fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, once: bool) -> i32;
    pub fn CFRunLoopStop(run_loop: CFRunLoopRef);
    pub fn CFRunLoopGetMain() -> CFRunLoopRef;

    pub fn dispatch_queue_create(label: *const c_char, attributes: *mut c_void) -> DispatchQueue;
    pub fn dispatch_queue_attr_make_with_qos_class(
        attributes: *mut c_void,
        class: u32,
        priority: i32,
    ) -> *mut c_void;
    pub fn dispatch_get_global_queue(identifier: isize, flags: usize) -> DispatchQueue;
}

/// `QOS_CLASS_USER_INTERACTIVE`, for the queue frames are handled on: the simulator's render
/// server waits on its callback.
pub const QOS_USER_INTERACTIVE: u32 = 0x21;
/// `QOS_CLASS_USER_INITIATED`, for replies the loop waits on.
pub const QOS_USER_INITIATED: isize = 0x19;

/// A serial queue of its own, for the life of the process.
pub fn serial_queue(label: &CStr, class: u32) -> DispatchQueue {
    // SAFETY: a label and a serial queue's attributes (none) made with a known class; the queue
    // is kept for as long as the process runs.
    unsafe {
        let attributes = dispatch_queue_attr_make_with_qos_class(std::ptr::null_mut(), class, 0);
        dispatch_queue_create(label.as_ptr(), attributes)
    }
}

/// The system's queue for work a caller waits on.
pub fn global_queue() -> DispatchQueue {
    // SAFETY: a global queue, which is never released.
    unsafe { dispatch_get_global_queue(QOS_USER_INITIATED, 0) }
}

/// A function of a system library that no header declares, found by name the first time it's
/// wanted and kept (the agent desktop host's `Symbol`): the lookup walks every loaded image.
pub struct Symbol {
    name: &'static CStr,
    found: std::sync::OnceLock<Option<usize>>,
}

impl Symbol {
    /// A symbol among everything loaded.
    pub const fn loaded(name: &'static CStr) -> Symbol {
        Symbol {
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
            // SAFETY: a zero-terminated name, looked up among the loaded images.
            let found = unsafe { libc::dlsym(libc::RTLD_DEFAULT, self.name.as_ptr()) };
            (!found.is_null()).then(|| found.expose_provenance())
        });
        found.map(|address| {
            let address: *mut c_void = std::ptr::with_exposed_provenance_mut(address);
            debug_assert_eq!(size_of::<F>(), size_of::<*mut c_void>());
            // SAFETY: the caller's promise; a function pointer and a data pointer are one size.
            unsafe { std::mem::transmute_copy(&address) }
        })
    }
}

/// Loads a framework the host reaches only by message, once; whether it's there.
pub fn load(path: &CStr) -> bool {
    // SAFETY: a zero-terminated path; the image stays loaded for the life of the process.
    unsafe { !libc::dlopen(path.as_ptr(), libc::RTLD_NOW).is_null() }
}

/// When `pid` started, in seconds since 1970: with the pid, what tells a process apart from a
/// later one given the same number.
pub fn started_at(pid: i32) -> Option<u64> {
    // SAFETY: a buffer of the size asked for, which the call fills.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
        let got = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size);
        (got == size).then_some(info.pbi_start_tvsec)
    }
}
