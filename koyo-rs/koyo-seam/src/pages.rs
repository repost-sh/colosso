//! The native pages there are, by the browser host's id of each: which views are a page's,
//! and which of them takes its keys (`docs/native-layer.md` 7). The browser host tells the
//! layer as it finds a page's views and as the page closes, and as the views leave a window or
//! come into one, which the keyboard acts on at once ([`leaving`], [`arrived`]).
//!
//! Until 2026-11-17 the app's keyboard asked the host for these each time, and heard of the
//! views moving through a function the host was handed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::msg_send;

use crate::keyboard::hooks;

/// The class of the view that takes a page's keys, as Chromium names it.
const KEYS: &CStr = c"RenderWidgetHostViewCocoa";

/// A page's views: the one its layers hang in, and the one over it that takes its input.
struct Views {
    compositor: Retained<AnyObject>,
    web: Retained<AnyObject>,
}

thread_local! {
    static PAGES: RefCell<HashMap<u64, Views>> = RefCell::new(HashMap::new());
}

/// `pane`'s page has these views from now on, wherever they are.
///
/// # Safety
/// `compositor` and `web` are live `NSView`s, and this is the main thread.
pub unsafe fn register(pane: u64, compositor: *mut AnyObject, web: *mut AnyObject) {
    // SAFETY: live objects, as the caller says, kept from now on.
    let views = unsafe { Retained::retain(compositor).zip(Retained::retain(web)) };
    if let Some((compositor, web)) = views {
        PAGES.with_borrow_mut(|pages| pages.insert(pane, Views { compositor, web }));
    }
}

/// `pane`'s page is gone, its views with it.
pub fn forget(pane: u64) {
    PAGES.with_borrow_mut(|pages| pages.remove(&pane));
}

/// Every page there is, lowest first.
#[must_use]
pub fn all() -> Vec<u64> {
    let mut pages: Vec<u64> = PAGES.with_borrow(|pages| pages.keys().copied().collect());
    pages.sort_unstable();
    pages
}

/// The page `view` is one of the views of, if it's a page's: its keys' view, or another in it.
///
/// # Safety
/// `view` is null or a live `NSView`, and this is the main thread.
#[must_use]
pub unsafe fn page_of(view: *mut AnyObject) -> Option<u64> {
    let view = unsafe { view.as_ref() }?;
    PAGES.with(|pages| {
        // Asked from inside a change of the pages too: then no page is anyone's.
        let pages = pages.try_borrow().ok()?;
        let within = |of: &AnyObject| -> bool {
            // SAFETY: both are live views.
            let within: Bool = unsafe { msg_send![view, isDescendantOf: of] };
            within.as_bool()
        };
        (pages.iter())
            .find(|(_, views)| within(&views.web) || within(&views.compositor))
            .map(|(pane, _)| *pane)
    })
}

/// The window `pane`'s page is in and the view of it that takes its keys, while it's in one.
#[must_use]
pub fn key_view(pane: u64) -> Option<(Retained<AnyObject>, Retained<AnyObject>)> {
    let web = PAGES.with_borrow(|pages| pages.get(&pane).map(|views| views.web.clone()))?;
    // SAFETY: a live view, asked for its window; the window, if any, kept.
    let window = unsafe {
        let window: *mut AnyObject = msg_send![&*web, window];
        Retained::retain(window)?
    };
    let keys = find(&web, KEYS).unwrap_or(web);
    Some((window, keys))
}

/// The first view at or under `root` whose class is named `class`. Asked with `[object class]`
/// and never read off the object: a view under key-value observing has a subclass there.
fn find(root: &AnyObject, class: &CStr) -> Option<Retained<AnyObject>> {
    // SAFETY: a live view, its class and its subviews read on the main thread.
    unsafe {
        let of: *const AnyClass = msg_send![root, class];
        if of.as_ref().is_some_and(|of| of.name() == class) {
            return Retained::retain(std::ptr::from_ref(root).cast_mut());
        }
        let children: *mut AnyObject = msg_send![root, subviews];
        let children = children.as_ref()?;
        let count: usize = msg_send![children, count];
        (0..count).find_map(|at| {
            let child: *mut AnyObject = msg_send![children, objectAtIndex: at];
            find(child.as_ref()?, class)
        })
    }
}

/// `pane`'s views are about to leave the window they're in, for another or for none, or the
/// page is about to close; told while the page is still known. A view that has the keyboard as
/// it leaves its window leaves the window itself with it, which hands no key on: the keyboard
/// is moved off them first.
pub fn leaving(pane: u64) {
    hooks::page_leaving(pane);
}

/// `pane`'s views are in a window now: a page the keyboard is wanted in takes it.
pub fn arrived(pane: u64) {
    hooks::page_arrived(pane);
}

#[cfg(test)]
mod tests {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    use super::{all, forget, key_view, page_of, register};

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    fn view() -> Retained<AnyObject> {
        // SAFETY: a plain view, made and kept by the test, in no window.
        unsafe { msg_send![class!(NSView), new] }
    }

    fn raw(view: &Retained<AnyObject>) -> *mut AnyObject {
        Retained::as_ptr(view).cast_mut()
    }

    fn add(parent: &Retained<AnyObject>, child: &Retained<AnyObject>) {
        // SAFETY: two live views, neither in a window.
        let _: () = unsafe { msg_send![&**parent, addSubview: &**child] };
    }

    #[test]
    fn a_view_is_its_pages_wherever_in_the_pages_views_it_is_and_nobodys_once_the_page_is_gone() {
        let (compositor, web, inner, deeper, other) = (view(), view(), view(), view(), view());
        add(&web, &inner);
        add(&inner, &deeper);
        // SAFETY: live views, in a test's own thread standing for the main one.
        unsafe {
            register(7, raw(&compositor), raw(&web));
            assert_eq!(page_of(raw(&web)), Some(7));
            assert_eq!(page_of(raw(&deeper)), Some(7), "a view deep in the page's");
            assert_eq!(page_of(raw(&compositor)), Some(7));
            assert_eq!(page_of(raw(&other)), None);
            assert_eq!(page_of(std::ptr::null_mut()), None);
        }
        assert_eq!(all(), vec![7]);
        assert!(key_view(7).is_none(), "in no window, it has no keys' view to give");
        forget(7);
        // SAFETY: as above.
        assert_eq!(unsafe { page_of(raw(&deeper)) }, None);
        assert!(all().is_empty());
    }

    #[test]
    fn each_view_is_the_page_it_is_in_and_no_others() {
        let (first, second) = ((view(), view()), (view(), view()));
        // SAFETY: live views.
        unsafe {
            register(1, raw(&first.0), raw(&first.1));
            register(2, raw(&second.0), raw(&second.1));
            assert_eq!(page_of(raw(&first.1)), Some(1));
            assert_eq!(page_of(raw(&second.0)), Some(2));
        }
        assert_eq!(all(), vec![1, 2]);
    }
}
