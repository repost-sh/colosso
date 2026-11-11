//! Messages to `CoreSimulator`'s remote proxies: its display ports and their descriptors are
//! objects of a forwarding class (`ROCKRemoteProxy-…`) whose method list doesn't name what they
//! forward, so `objc2`'s debug check of a message's types, which looks the method up there, can't
//! be used on them. These send through `objc_msgSend` with the exact types the headers give
//! (`SimDisplayIOSurfaceRenderable`, `SimScreen`, `SimDisplayDescriptorState`); everything else
//! stays checked.

use std::ffi::c_void;

use objc2::runtime::{AnyObject, Sel};

unsafe extern "C" {
    fn objc_msgSend();
}

/// `objc_msgSend` as the function type `F` of the method sent.
///
/// # Safety
/// `F` must be the method's own C type, receiver and selector first.
unsafe fn send<F: Copy>() -> F {
    let function: unsafe extern "C" fn() = objc_msgSend;
    debug_assert_eq!(size_of::<F>(), size_of::<unsafe extern "C" fn()>());
    // SAFETY: the caller's promise; a function pointer of one C type as another of the same size.
    unsafe { std::mem::transmute_copy(&function) }
}

/// `[receiver selector]` for a method that returns an object (unretained) or nil.
///
/// # Safety
/// `receiver` must be live and its `selector` a method that takes nothing and returns an object.
pub unsafe fn object(receiver: *mut AnyObject, selector: Sel) -> *mut AnyObject {
    // SAFETY: the caller's promise.
    unsafe {
        send::<unsafe extern "C" fn(*mut AnyObject, Sel) -> *mut AnyObject>()(receiver, selector)
    }
}

/// `[receiver selector]` for a method that returns an `unsigned short`.
///
/// # Safety
/// As [`object`], for a method returning `unsigned short`.
pub unsafe fn short(receiver: *mut AnyObject, selector: Sel) -> u16 {
    // SAFETY: the caller's promise.
    unsafe { send::<unsafe extern "C" fn(*mut AnyObject, Sel) -> u16>()(receiver, selector) }
}

/// `[receiver selector:argument]` for a method that takes one object and returns nothing.
///
/// # Safety
/// As [`object`], for a method taking one object and returning `void`.
pub unsafe fn with(receiver: *mut AnyObject, selector: Sel, argument: *mut AnyObject) {
    // SAFETY: the caller's promise.
    unsafe {
        send::<unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject)>()(
            receiver, selector, argument,
        );
    }
}

/// `registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:
/// propertiesChangedCallback:`, as `SimScreen` declares it.
///
/// # Safety
/// `receiver` must be a live screen; the blocks must be blocks of the declared types.
pub unsafe fn register(
    receiver: *mut AnyObject,
    uuid: *mut AnyObject,
    queue: *mut c_void,
    blocks: [*const c_void; 3],
) {
    type Register = unsafe extern "C" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut c_void,
        *const c_void,
        *const c_void,
        *const c_void,
    );
    let selector = objc2::sel!(
        registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:propertiesChangedCallback:
    );
    // SAFETY: the caller's promise.
    unsafe {
        send::<Register>()(
            receiver, selector, uuid, queue, blocks[0], blocks[1], blocks[2],
        );
    }
}
