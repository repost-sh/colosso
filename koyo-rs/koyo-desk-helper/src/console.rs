//! Who is at the Mac (`docs/agent-desktop.md` 6.1): the kernel's console users
//! (`IOConsoleUsers` on the I/O Registry's root), one entry per GUI session with its user and
//! whether it's the one on the console. Not `/dev/console`'s owner nor
//! `State:/Users/ConsoleUser`'s `Name` and `UID`: an agent's login takes both over while the
//! person stays at the console, and a killed session leaves them so. Read in process through
//! `IOKit` (a few microseconds), as the console watch looks every two seconds.

use std::ffi::{CStr, c_char, c_void};

/// A GUI session as the kernel lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub uid: Option<u32>,
    pub on_console: bool,
}

type CFTypeRef = *const c_void;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegistryGetRootEntry(main_port: u32) -> u32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: CFTypeRef,
        allocator: CFTypeRef,
        options: u32,
    ) -> CFTypeRef;
    fn IOObjectRelease(object: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        text: *const c_char,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFGetTypeID(value: CFTypeRef) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFArrayGetCount(array: CFTypeRef) -> isize;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, kind: isize, value: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFRelease(value: CFTypeRef);
}

const UTF8: u32 = 0x0800_0100;
const SINT64: isize = 4;

/// A `CFString` made from `text`, released when dropped.
struct Key(CFTypeRef);

impl Key {
    fn new(text: &CStr) -> Option<Key> {
        // SAFETY: a zero-terminated string; the result is ours to release.
        let made = unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) };
        (!made.is_null()).then_some(Key(made))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: made by `new`, released once.
        unsafe { CFRelease(self.0) };
    }
}

/// `dictionary[key]` when it's of the type `type_id` names.
///
/// # Safety
/// `dictionary` must be a live `CFDictionary`.
unsafe fn get(dictionary: CFTypeRef, key: &Key, type_id: usize) -> Option<CFTypeRef> {
    // SAFETY: the caller's dictionary and our key; a value got isn't owned.
    let value = unsafe { CFDictionaryGetValue(dictionary, key.0) };
    // SAFETY: a non-null CF object.
    (!value.is_null() && unsafe { CFGetTypeID(value) } == type_id).then_some(value)
}

/// The GUI sessions the kernel knows; none when they can't be read.
pub fn read() -> Vec<Session> {
    let (Some(users), Some(uid_key), Some(console_key)) = (
        Key::new(c"IOConsoleUsers"),
        Key::new(c"kCGSSessionUserIDKey"),
        Key::new(c"kCGSSessionOnConsoleKey"),
    ) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    // SAFETY: the registry's root (the default main port is 0), a property copied for us and
    // released below, and values read only while it lives.
    unsafe {
        let root = IORegistryGetRootEntry(0);
        let list = IORegistryEntryCreateCFProperty(root, users.0, std::ptr::null(), 0);
        IOObjectRelease(root);
        if list.is_null() {
            return sessions;
        }
        if CFGetTypeID(list) == CFArrayGetTypeID() {
            for index in 0..CFArrayGetCount(list) {
                let session = CFArrayGetValueAtIndex(list, index);
                if session.is_null() || CFGetTypeID(session) != CFDictionaryGetTypeID() {
                    continue;
                }
                let uid = get(session, &uid_key, CFNumberGetTypeID()).and_then(|number| {
                    let mut value = 0i64;
                    (CFNumberGetValue(number, SINT64, (&raw mut value).cast()) != 0)
                        .then(|| u32::try_from(value).ok())
                        .flatten()
                });
                let on_console = get(session, &console_key, CFBooleanGetTypeID())
                    .is_some_and(|flag| CFBooleanGetValue(flag) != 0);
                sessions.push(Session { uid, on_console });
            }
        }
        CFRelease(list);
    }
    sessions
}

/// Whether `uid`'s session is the one on the console.
pub fn on_console(sessions: &[Session], uid: u32) -> bool {
    sessions
        .iter()
        .any(|session| session.on_console && session.uid == Some(uid))
}

/// Whether the kernel lists a session of `uid`.
pub fn listed(sessions: &[Session], uid: u32) -> bool {
    sessions.iter().any(|session| session.uid == Some(uid))
}

/// Whether `uid` has a session and every one of them is off the console: the only sessions the
/// helper may ever log out.
pub fn only_off_console(sessions: &[Session], uid: u32) -> bool {
    listed(sessions, uid)
        && sessions
            .iter()
            .filter(|session| session.uid == Some(uid))
            .all(|session| !session.on_console)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What 27.2 listed with the person at the console and an agent logged in off it.
    fn listed_now() -> Vec<Session> {
        vec![
            Session {
                uid: Some(501),
                on_console: true,
            },
            Session {
                uid: Some(506),
                on_console: false,
            },
        ]
    }

    #[test]
    fn the_person_is_at_the_console_and_the_agent_off_it() {
        let sessions = listed_now();
        assert!(on_console(&sessions, 501));
        assert!(!on_console(&sessions, 506));
        assert!(only_off_console(&sessions, 506));
        assert!(
            !only_off_console(&sessions, 501),
            "the person's session is never logged out"
        );
        assert!(
            !only_off_console(&sessions, 999),
            "no session, nothing to log out"
        );
        assert!(listed(&sessions, 506) && !listed(&sessions, 999));
    }

    #[test]
    fn the_login_window_has_nobody_on_the_console() {
        let sessions = vec![Session {
            uid: None,
            on_console: true,
        }];
        assert!(!on_console(&sessions, 501));
    }

    #[test]
    fn the_registry_reads_without_fault() {
        // Read-only: the registry's property, as any user may read it; a Mac with nobody
        // logged in lists no one.
        for session in read() {
            assert!(session.uid.is_some() || session.on_console, "{session:?}");
        }
    }
}
