//! Core Foundation and Foundation values as the host handles them: one retained object type,
//! read by message. Core Foundation's objects answer Objective-C messages (strings, numbers,
//! arrays and dictionaries are the same objects on both sides), so one handful of readers
//! serves the window list, accessibility and the workspace alike.

use std::ffi::CStr;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::NSString;

use crate::sys::{self, Ref};

/// Any object, kept alive while held.
pub type Obj = Retained<AnyObject>;

/// A completion handler nobody waits on: a nil block, of the kind that takes `Arguments`.
pub fn no_handler<Arguments>() -> Option<&'static block2::Block<Arguments>>
where
    Arguments: ?Sized,
{
    None
}

/// The number that says yes.
pub fn yes() -> Obj {
    // SAFETY: `NSNumber`'s own constructor.
    unsafe { msg_send![class!(NSNumber), numberWithBool: true] }
}

/// The number that says no.
pub fn no() -> Obj {
    // SAFETY: `NSNumber`'s own constructor.
    unsafe { msg_send![class!(NSNumber), numberWithBool: false] }
}

/// Takes over a reference a `Create` or `Copy` call handed out.
pub fn owned(object: Ref) -> Option<Obj> {
    // SAFETY: the caller got `object` from a call that hands out one reference, which this
    // takes; every Core Foundation object is an Objective-C object.
    unsafe { Retained::from_raw(object.cast_mut().cast()) }
}

/// Keeps an object a `Get` call lent.
pub fn kept(object: Ref) -> Option<Obj> {
    // SAFETY: `object` is null or a live object, retained here.
    unsafe { Retained::retain(object.cast_mut().cast()) }
}

/// The object as Core Foundation's calls take it.
pub fn raw(object: &AnyObject) -> Ref {
    std::ptr::from_ref(object).cast()
}

pub fn string(text: &str) -> Obj {
    Retained::into_super(Retained::into_super(NSString::from_str(text)))
}

pub fn number(value: i64) -> Obj {
    // SAFETY: `NSNumber`'s own constructor.
    unsafe { msg_send![class!(NSNumber), numberWithLongLong: value] }
}

fn is_kind(object: &AnyObject, class: &objc2::runtime::AnyClass) -> bool {
    // SAFETY: every object answers `isKindOfClass:`.
    unsafe { msg_send![object, isKindOfClass: class] }
}

/// The text of a string object; `None` for anything else.
pub fn text(object: &AnyObject) -> Option<String> {
    if !is_kind(object, class!(NSString)) {
        return None;
    }
    // SAFETY: a string, whose UTF-8 bytes live until the autorelease pool drains; copied here.
    let bytes: *const std::ffi::c_char = unsafe { msg_send![object, UTF8String] };
    if bytes.is_null() {
        return None;
    }
    // SAFETY: zero-terminated, as `UTF8String` promises.
    Some(
        unsafe { CStr::from_ptr(bytes) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// A number object's value; `None` for anything else.
pub fn integer(object: &AnyObject) -> Option<i64> {
    // SAFETY: a number answers `longLongValue`.
    is_kind(object, class!(NSNumber)).then(|| unsafe { msg_send![object, longLongValue] })
}

pub fn float(object: &AnyObject) -> Option<f64> {
    // SAFETY: a number answers `doubleValue`.
    is_kind(object, class!(NSNumber)).then(|| unsafe { msg_send![object, doubleValue] })
}

/// An array object's items; none for anything else.
pub fn items(object: &AnyObject) -> Vec<Obj> {
    if !is_kind(object, class!(NSArray)) {
        return Vec::new();
    }
    // SAFETY: an array answers `count` and `objectAtIndex:` below its count.
    unsafe {
        let count: usize = msg_send![object, count];
        (0..count)
            .filter_map(|index| msg_send![object, objectAtIndex: index])
            .collect()
    }
}

/// An array object of `items`, which it keeps.
pub fn array(items: &[Obj]) -> Obj {
    let pointers: Vec<*const AnyObject> = items.iter().map(|item| &raw const **item).collect();
    // SAFETY: `NSArray`'s own constructor over live objects, which it retains.
    unsafe {
        msg_send![class!(NSArray), arrayWithObjects: pointers.as_ptr(), count: pointers.len()]
    }
}

/// The whole numbers in a list a call handed out, those that fit `T`; none for no list.
pub fn numbers<T: TryFrom<i64>>(list: Option<Obj>) -> Vec<T> {
    let Some(list) = list else {
        return Vec::new();
    };
    (items(&list).iter())
        .filter_map(|number| T::try_from(integer(number)?).ok())
        .collect()
}

/// `dictionary[key]`; `None` when it isn't a dictionary or has no such key.
pub fn entry(dictionary: &AnyObject, key: &str) -> Option<Obj> {
    if !is_kind(dictionary, class!(NSDictionary)) {
        return None;
    }
    // SAFETY: a dictionary answers `objectForKey:` with a string key.
    unsafe { msg_send![dictionary, objectForKey: &*string(key)] }
}

/// Runs this thread's run loop for `time`, so observers, notices and completion handlers that
/// come to it are served. Returns early when [`wake`] is called.
pub fn pump(time: Duration) {
    // SAFETY: the default mode, on the calling thread's run loop.
    unsafe { sys::CFRunLoopRunInMode(sys::kCFRunLoopDefaultMode, time.as_secs_f64(), false) };
}

/// Pumps until `done` says so or `limit` has passed; whether it did.
pub fn pump_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    loop {
        if done() {
            return true;
        }
        if started.elapsed() >= limit {
            return false;
        }
        pump(Duration::from_millis(5));
    }
}

/// Pumps for all of `time`, whoever wakes the loop meanwhile: the gap between two events
/// of one act, which an app reads meaning into.
pub fn pause(time: Duration) {
    let started = Instant::now();
    while let Some(left) = time.checked_sub(started.elapsed()) {
        if left.is_zero() {
            break;
        }
        pump(left);
    }
}

/// Ends the main thread's current [`pump`] at once: something came for it.
pub fn wake() {
    // SAFETY: stopping a run loop is allowed from any thread.
    unsafe { sys::CFRunLoopStop(sys::CFRunLoopGetMain()) };
}
