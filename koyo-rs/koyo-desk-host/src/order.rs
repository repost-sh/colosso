//! The running apps in front-to-back order, as Launch Services keeps it (private): which app
//! the person had in front the moment an agent's app took it (`docs/agent-desktop.md` 2).
//! Asked when it's needed, not remembered: the workspace tells of one activation when two
//! follow within milliseconds, so the app it named last can be one behind.

use std::ffi::CStr;
use std::sync::OnceLock;

use crate::sys::{self, Ref};

const LIBRARY: &CStr = c"/System/Library/Frameworks/CoreServices.framework/CoreServices";
/// The session the caller is in.
const THIS_SESSION: i32 = -2;
/// `kCFNumberSInt32Type`.
const INT32: isize = 3;

struct Calls {
    in_order: unsafe extern "C" fn(i32) -> Ref,
    item: unsafe extern "C" fn(i32, Ref, Ref) -> Ref,
    pid_key: Ref,
}

// SAFETY: two functions and a constant string of a system library, all of which any thread
// may use.
unsafe impl Send for Calls {}
// SAFETY: as above.
unsafe impl Sync for Calls {}

fn calls() -> Option<&'static Calls> {
    static CALLS: OnceLock<Option<Calls>> = OnceLock::new();
    CALLS
        .get_or_init(|| {
            let find = |name: &CStr| sys::symbol(LIBRARY, name);
            // SAFETY: each symbol is the function, or the constant, of that name in Launch
            // Services, with the signature it has been called by for years; checked against
            // this macOS (the app switcher's order came back as it stood).
            unsafe {
                Some(Calls {
                    in_order: sys::function(find(c"_LSCopyApplicationArrayInFrontToBackOrder")?),
                    item: sys::function(find(c"_LSCopyApplicationInformationItem")?),
                    pid_key: *find(c"_kLSPIDKey")?.cast::<Ref>(),
                })
            }
        })
        .as_ref()
}

/// The pids of the running apps that have a place in the app switcher, front to back. Empty
/// when Launch Services' calls aren't there.
pub fn pids() -> Vec<i32> {
    let Some(calls) = calls() else {
        return Vec::new();
    };
    // SAFETY: the array and each item are copies this releases; an item that isn't there is
    // null and skipped; the number is read as the 32-bit integer a pid is.
    unsafe {
        let apps = (calls.in_order)(THIS_SESSION);
        if apps.is_null() {
            return Vec::new();
        }
        let mut pids = Vec::new();
        for index in 0..sys::CFArrayGetCount(apps) {
            let app = sys::CFArrayGetValueAtIndex(apps, index);
            let number = (calls.item)(THIS_SESSION, app, calls.pid_key);
            if number.is_null() {
                continue;
            }
            let mut pid = 0i32;
            if sys::CFNumberGetValue(number, INT32, (&raw mut pid).cast()) {
                pids.push(pid);
            }
            sys::CFRelease(number);
        }
        sys::CFRelease(apps);
        pids
    }
}

/// The app the person had in front: the first, front to back, that isn't the agent's.
pub fn persons_among(order: &[i32], ours: &[i32]) -> Option<i32> {
    order.iter().copied().find(|pid| !ours.contains(pid))
}

#[cfg(test)]
mod tests {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    use super::{persons_among, pids};

    /// At a Mac with somebody logged in: the order Launch Services gives starts with the app
    /// the workspace calls the front one, and names each app once.
    #[test]
    #[ignore = "needs a session at a Mac"]
    fn the_order_starts_with_the_front_app() {
        let order = pids();
        // SAFETY: the shared workspace and its front app, which answers its pid.
        let front: i32 = unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
            assert!(!app.is_null(), "no front app");
            msg_send![app, processIdentifier]
        };
        println!("front to back: {order:?}; the workspace's front app: {front}");
        assert_eq!(order.first(), Some(&front));
        let mut once = order.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(once.len(), order.len(), "an app named twice");
    }

    /// The agent's app that just took the front is passed over, and so is another of the
    /// agent's behind it: the person's is the first that's left, whatever was front before.
    #[test]
    fn the_persons_app_is_the_first_in_order_that_isnt_the_agents() {
        assert_eq!(persons_among(&[70, 12, 34], &[70]), Some(12));
        assert_eq!(persons_among(&[70, 71, 34, 12], &[70, 71]), Some(34));
        assert_eq!(persons_among(&[12, 34], &[70]), Some(12));
        assert_eq!(persons_among(&[70], &[70]), None);
        assert_eq!(persons_among(&[], &[70]), None);
    }
}
