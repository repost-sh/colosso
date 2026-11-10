//! Accessibility: the elements of other apps, read and acted on (`docs/agent-desktop.md` 7).
//! Every call to another app is given a short timeout, so a hung app costs a fraction of a
//! second, not the host.

use std::ffi::c_void;

use objc2_core_foundation::{CGPoint, CGSize};

use koyo_desk_wire::Rect;

use crate::ns::{self, Obj};
use crate::sys::{self, Ref};

/// An accessibility element, kept alive while held.
#[derive(Clone)]
pub struct Element(Obj);

/// How long another app gets to answer.
const ANSWER_WITHIN: f32 = 1.0;
/// `kAXErrorCannotComplete` and `kAXErrorInvalidUIElement`.
const CANNOT_COMPLETE: i32 = -25204;
const INVALID_ELEMENT: i32 = -25202;

/// How an action went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Yes,
    /// The element doesn't take the action.
    No,
    /// The app didn't answer in time: an action that opens something modal answers only when
    /// that closes, so it was most likely taken.
    NoAnswer,
    /// The element isn't there any more.
    Gone,
}

impl Element {
    /// The app `pid` as an element.
    pub fn app(pid: i32) -> Option<Element> {
        // SAFETY: creates an element for any pid; it's ours to release.
        let element = ns::owned(unsafe { sys::AXUIElementCreateApplication(pid) })?;
        // SAFETY: a live element.
        unsafe { sys::AXUIElementSetMessagingTimeout(ns::raw(&element), ANSWER_WITHIN) };
        Some(Element(element))
    }

    /// The window server's number for the window this element is, by the call window
    /// managers use (`_AXUIElementGetWindow`, private).
    pub fn window_id(&self) -> Option<u32> {
        type Get = unsafe extern "C" fn(Ref, *mut u32) -> i32;
        static GET: sys::Symbol = sys::Symbol::loaded(c"_AXUIElementGetWindow");
        // SAFETY: the symbol is that function; a live element and an out-pointer.
        let (error, id) = unsafe {
            let get = GET.function::<Get>()?;
            let mut id = 0u32;
            (get(self.raw(), &raw mut id), id)
        };
        (error == 0 && id != 0).then_some(id)
    }

    pub fn raw(&self) -> Ref {
        ns::raw(&self.0)
    }

    /// The attribute's value, whatever it is.
    pub fn get(&self, attribute: &str) -> Option<Obj> {
        let mut value: Ref = std::ptr::null();
        // SAFETY: a live element, a string attribute, and an out-pointer whose reference is ours.
        let error = unsafe {
            sys::AXUIElementCopyAttributeValue(
                self.raw(),
                ns::raw(&ns::string(attribute)),
                &raw mut value,
            )
        };
        if error == 0 { ns::owned(value) } else { None }
    }

    pub fn text(&self, attribute: &str) -> Option<String> {
        ns::text(&*self.get(attribute)?)
    }

    pub fn element(&self, attribute: &str) -> Option<Element> {
        self.get(attribute).map(Element)
    }

    pub fn elements(&self, attribute: &str) -> Vec<Element> {
        self.get(attribute)
            .map(|list| ns::items(&list).into_iter().map(Element).collect())
            .unwrap_or_default()
    }

    pub fn children(&self) -> Vec<Element> {
        self.elements("AXChildren")
    }

    pub fn point(&self, attribute: &str) -> Option<CGPoint> {
        as_point(&self.get(attribute)?)
    }

    pub fn size(&self, attribute: &str) -> Option<CGSize> {
        as_size(&self.get(attribute)?)
    }

    /// Where the element is on the desktop.
    pub fn frame(&self) -> Option<Rect> {
        let (at, size) = (self.point("AXPosition")?, self.size("AXSize")?);
        Some(Rect {
            x: at.x,
            y: at.y,
            width: size.width,
            height: size.height,
        })
    }

    /// Several attributes in one call to the app, each `None` where the element has none.
    pub fn many(&self, attributes: &[&str]) -> Vec<Option<Obj>> {
        let names: Vec<Obj> = attributes.iter().map(|name| ns::string(name)).collect();
        let pointers: Vec<*const c_void> = names.iter().map(|name| ns::raw(name)).collect();
        let mut values: Ref = std::ptr::null();
        // SAFETY: an array of strings that live in `names` for the call (it has no callbacks
        // to retain them), a live element, and an out-pointer whose reference is ours.
        let error = unsafe {
            let wanted = sys::CFArrayCreate(
                std::ptr::null(),
                pointers.as_ptr(),
                pointers.len().cast_signed(),
                std::ptr::null(),
            );
            let error =
                sys::AXUIElementCopyMultipleAttributeValues(self.raw(), wanted, 0, &raw mut values);
            if !wanted.is_null() {
                sys::CFRelease(wanted);
            }
            error
        };
        let read = if error == 0 { ns::owned(values) } else { None };
        // An attribute the element hasn't comes back as an error value, which is neither
        // text, number, array nor point: readers of each kind take it for nothing.
        let mut read: Vec<Option<Obj>> = read
            .map(|values| ns::items(&values).into_iter().map(Some).collect())
            .unwrap_or_default();
        read.resize_with(attributes.len(), || None);
        read
    }

    /// The element a process numbers `id`, by the token accessibility itself passes around
    /// (`_AXUIElementCreateWithRemoteToken`, private): the way to a window the app doesn't
    /// list because its desktop isn't showing.
    pub fn numbered(pid: i32, id: u64) -> Option<Element> {
        type Create = unsafe extern "C" fn(Ref) -> Ref;
        static CREATE: sys::Symbol = sys::Symbol::loaded(c"_AXUIElementCreateWithRemoteToken");
        let mut token = [0u8; 20];
        token[0..4].copy_from_slice(&pid.to_ne_bytes());
        token[8..12].copy_from_slice(&0x636f_636f_i32.to_ne_bytes());
        token[12..20].copy_from_slice(&id.to_ne_bytes());
        // SAFETY: the symbol is that function; it takes data it copies and hands back an
        // element that is ours.
        let element = unsafe {
            let create = CREATE.function::<Create>()?;
            let data = ns::owned(sys::CFDataCreate(std::ptr::null(), token.as_ptr(), 20))?;
            ns::owned(create(ns::raw(&data)))?
        };
        // SAFETY: a live element.
        unsafe { sys::AXUIElementSetMessagingTimeout(ns::raw(&element), ANSWER_WITHIN) };
        Some(Element(element))
    }

    /// Sets an attribute; whether the app took it.
    pub fn set(&self, attribute: &str, value: &Obj) -> bool {
        // SAFETY: a live element, a string attribute and a live value.
        let error = unsafe {
            sys::AXUIElementSetAttributeValue(
                self.raw(),
                ns::raw(&ns::string(attribute)),
                ns::raw(value),
            )
        };
        error == 0
    }

    pub fn set_point(&self, attribute: &str, point: CGPoint) -> bool {
        // SAFETY: a value made from a point of the type named; ours to release.
        let value =
            unsafe { sys::AXValueCreate(sys::AX_POINT, (&raw const point).cast::<c_void>()) };
        ns::owned(value).is_some_and(|value| self.set(attribute, &value))
    }

    pub fn set_size(&self, attribute: &str, size: CGSize) -> bool {
        // SAFETY: as in `set_point`.
        let value = unsafe { sys::AXValueCreate(sys::AX_SIZE, (&raw const size).cast::<c_void>()) };
        ns::owned(value).is_some_and(|value| self.set(attribute, &value))
    }

    /// Performs an action; whether the app took it.
    pub fn perform(&self, action: &str) -> bool {
        self.act(action) == Done::Yes
    }

    /// Performs an action and says how it went.
    pub fn act(&self, action: &str) -> Done {
        // SAFETY: a live element and a string action.
        let error =
            unsafe { sys::AXUIElementPerformAction(self.raw(), ns::raw(&ns::string(action))) };
        match error {
            0 => Done::Yes,
            CANNOT_COMPLETE => Done::NoAnswer,
            INVALID_ELEMENT => Done::Gone,
            _ => Done::No,
        }
    }

    /// The first element under this one for which `wanted` holds, looking at no more than
    /// `budget` elements and no deeper than `depth`.
    pub fn find(
        &self,
        depth: usize,
        budget: &mut usize,
        wanted: &impl Fn(&Element) -> bool,
    ) -> Option<Element> {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        if wanted(self) {
            return Some(self.clone());
        }
        if depth == 0 {
            return None;
        }
        self.children()
            .iter()
            .find_map(|child| child.find(depth - 1, budget, wanted))
    }

    /// Every element under this one with the identifier `identifier`, not looking inside one
    /// that has it.
    pub fn all_with_identifier(&self, identifier: &str) -> Vec<Element> {
        fn gather(
            element: &Element,
            identifier: &str,
            depth: usize,
            budget: &mut usize,
            found: &mut Vec<Element>,
        ) {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            if element.text("AXIdentifier").as_deref() == Some(identifier) {
                found.push(element.clone());
                return;
            }
            if depth > 0 {
                for child in element.children() {
                    gather(&child, identifier, depth - 1, budget, found);
                }
            }
        }
        let (mut budget, mut found) = (4000, Vec::new());
        gather(self, identifier, 14, &mut budget, &mut found);
        found
    }

    /// The first element under this one with the identifier `identifier`.
    pub fn with_identifier(&self, identifier: &str) -> Option<Element> {
        let mut budget = 4000;
        self.find(14, &mut budget, &|element| {
            element.text("AXIdentifier").as_deref() == Some(identifier)
        })
    }
}

/// Whether the object is an accessibility value (a point, a size, a range, an error).
fn is_value(object: &Obj) -> bool {
    // SAFETY: any Core Foundation object has a type id.
    unsafe { sys::CFGetTypeID(ns::raw(object)) == sys::AXValueGetTypeID() }
}

/// Gives every app a second to answer any accessibility call of this process, whichever
/// element it's made on: set on the system-wide element, it's the default for them all.
/// A hung app then costs the host's one loop a second, not the six macOS allows.
pub fn answer_within_a_second() {
    // SAFETY: the system-wide element, ours to release.
    if let Some(system) = ns::owned(unsafe { sys::AXUIElementCreateSystemWide() }) {
        // SAFETY: a live element.
        unsafe { sys::AXUIElementSetMessagingTimeout(ns::raw(&system), ANSWER_WITHIN) };
    }
}

/// A point out of an accessibility value; `None` for any other value.
pub fn as_point(value: &Obj) -> Option<CGPoint> {
    if !is_value(value) {
        return None;
    }
    let mut point = CGPoint { x: 0.0, y: 0.0 };
    // SAFETY: an out-pointer of the type asked for; the call refuses a value of another.
    let read =
        unsafe { sys::AXValueGetValue(ns::raw(value), sys::AX_POINT, (&raw mut point).cast()) };
    (read != 0).then_some(point)
}

/// A size out of an accessibility value; `None` for any other value.
pub fn as_size(value: &Obj) -> Option<CGSize> {
    if !is_value(value) {
        return None;
    }
    let mut size = CGSize {
        width: 0.0,
        height: 0.0,
    };
    // SAFETY: as in `as_point`.
    let read =
        unsafe { sys::AXValueGetValue(ns::raw(value), sys::AX_SIZE, (&raw mut size).cast()) };
    (read != 0).then_some(size)
}

impl From<Obj> for Element {
    fn from(element: Obj) -> Element {
        Element(element)
    }
}

/// Watches one app for a notice (`AXWindowCreated`…), calling back on the main run loop with
/// the element it's about.
pub struct Observer {
    observer: Obj,
    /// The callback's context, freed when the observer goes.
    context: *mut Callback,
}

type Callback = Box<dyn Fn(Element)>;

unsafe extern "C" fn observed(_observer: Ref, element: Ref, _notice: Ref, context: *mut c_void) {
    // SAFETY: `context` is the callback `Observer::new` boxed, alive as long as the observer;
    // `element` is lent for the call and kept here.
    let callback = unsafe { &*context.cast::<Callback>() };
    if let Some(element) = ns::kept(element) {
        callback(Element(element));
    }
}

impl Observer {
    /// Starts watching `pid` for `notice`; `None` when the app can't be watched.
    pub fn new(pid: i32, notice: &str, callback: impl Fn(Element) + 'static) -> Option<Observer> {
        let app = Element::app(pid)?;
        let mut observer: Ref = std::ptr::null();
        // SAFETY: an out-pointer whose reference is ours.
        if unsafe { sys::AXObserverCreate(pid, observed, &raw mut observer) } != 0 {
            return None;
        }
        let observer = ns::owned(observer)?;
        let callback: Callback = Box::new(callback);
        let context = Box::into_raw(Box::new(callback));
        // SAFETY: live observer and element; the context outlives the observer (`drop`); the
        // source is lent, and added to the main run loop, where the host pumps.
        unsafe {
            let added = sys::AXObserverAddNotification(
                ns::raw(&observer),
                app.raw(),
                ns::raw(&ns::string(notice)),
                context.cast(),
            );
            if added != 0 {
                drop(Box::from_raw(context));
                return None;
            }
            sys::CFRunLoopAddSource(
                sys::CFRunLoopGetCurrent(),
                sys::AXObserverGetRunLoopSource(ns::raw(&observer)),
                sys::kCFRunLoopDefaultMode,
            );
        }
        Some(Observer { observer, context })
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        // The run loop keeps the observer's source for itself: taken off it first, so no
        // notice comes for a context that is freed. Made on the main run loop, dropped there.
        // SAFETY: the observer is live; its source is lent; the context was made by
        // `Box::into_raw` in `new` and is freed once, after nothing can call it.
        unsafe {
            sys::CFRunLoopRemoveSource(
                sys::CFRunLoopGetMain(),
                sys::AXObserverGetRunLoopSource(ns::raw(&self.observer)),
                sys::kCFRunLoopDefaultMode,
            );
            drop(Box::from_raw(self.context));
        }
    }
}
