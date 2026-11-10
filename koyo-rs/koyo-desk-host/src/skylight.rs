//! The window server's own calls (`SkyLight`, private): which desktops there are, which
//! windows a desktop holds, and assigning an app to a desktop, as the Dock's "Assign To: This
//! Desktop" does. All of them work from a normal process with System Integrity Protection on
//! (`docs/agent-desktop.md` 4, 5); none moves a single window, which the window server keeps
//! to the Dock.

use std::ffi::CStr;
use std::sync::OnceLock;

use crate::ns::{self, Obj};
use crate::sys::{self, Ref};

/// A desktop (a Space), as the window server numbers it.
pub type SpaceId = u64;

pub const LIBRARY: &CStr = c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";

struct Calls {
    connection: unsafe extern "C" fn() -> i32,
    managed_spaces: unsafe extern "C" fn(i32) -> Ref,
    active_space: unsafe extern "C" fn(i32) -> u64,
    windows: unsafe extern "C" fn(i32, u32, Ref, u32, *mut u64, *mut u64) -> Ref,
    assign: unsafe extern "C" fn(i32, i32, u64) -> i32,
    spaces_of: unsafe extern "C" fn(i32, i32, Ref) -> Ref,
}

fn calls() -> Option<&'static Calls> {
    static CALLS: OnceLock<Option<Calls>> = OnceLock::new();
    CALLS
        .get_or_init(|| {
            let find = |name: &CStr| sys::symbol(LIBRARY, name);
            // SAFETY: each symbol is the function of that name in SkyLight, with the
            // signature window managers have called it by for years; checked against this
            // macOS in the lab (`INSESSION-27.md`).
            unsafe {
                Some(Calls {
                    connection: sys::function(find(c"SLSMainConnectionID")?),
                    managed_spaces: sys::function(find(c"SLSCopyManagedDisplaySpaces")?),
                    active_space: sys::function(find(c"SLSGetActiveSpace")?),
                    windows: sys::function(find(c"SLSCopyWindowsWithOptionsAndTags")?),
                    assign: sys::function(find(c"SLSProcessAssignToSpace")?),
                    spaces_of: sys::function(find(c"SLSCopySpacesForWindows")?),
                })
            }
        })
        .as_ref()
}

/// Whether the window server's calls were found.
pub fn available() -> bool {
    calls().is_some()
}

/// A desktop as the window server lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Space {
    pub id: SpaceId,
    /// The name (a UUID) it keeps across restarts.
    pub name: String,
    /// The display it's on, by the display's own UUID; `Main` when all displays share one
    /// set of desktops.
    pub display: String,
    /// An app's full screen, not a desktop.
    pub full_screen: bool,
}

/// The window server's kind of space that is an app's full screen.
const FULL_SCREEN: i64 = 4;

/// The person's desktops, in Mission Control's order, across displays.
pub fn desktops() -> Vec<SpaceId> {
    spaces().into_iter().map(|space| space.id).collect()
}

/// The person's desktops with their names and displays, display by display.
pub fn spaces() -> Vec<Space> {
    let Some(calls) = calls() else {
        return Vec::new();
    };
    // SAFETY: a copy the caller owns, for this process's own connection.
    let displays = ns::owned(unsafe { (calls.managed_spaces)((calls.connection)()) });
    let Some(displays) = displays else {
        return Vec::new();
    };
    let mut listed = Vec::new();
    for display in ns::items(&displays) {
        let identifier = ns::entry(&display, "Display Identifier")
            .and_then(|identifier| ns::text(&identifier))
            .unwrap_or_default();
        let spaces = ns::entry(&display, "Spaces")
            .map(|spaces| ns::items(&spaces))
            .unwrap_or_default();
        listed.extend(spaces.iter().filter_map(|space| {
            let id = ns::integer(&*ns::entry(space, "ManagedSpaceID")?)?;
            let name = ns::entry(space, "uuid").and_then(|name| ns::text(&name));
            let kind = ns::entry(space, "type").and_then(|kind| ns::integer(&kind));
            Some(Space {
                id: SpaceId::try_from(id).ok()?,
                name: name.unwrap_or_default(),
                display: identifier.clone(),
                full_screen: kind == Some(FULL_SCREEN),
            })
        }));
    }
    listed
}

/// A process's serial number, as the window server's older calls name a process.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Serial {
    high: u32,
    low: u32,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn GetProcessForPID(pid: i32, serial: *mut Serial) -> i32;
    fn GetProcessPID(serial: *const Serial, pid: *mut i32) -> i32;
}

/// The app the window server has in front, by its serial number. Any thread may ask.
pub fn front_serial() -> Option<Serial> {
    type Front = unsafe extern "C" fn(*mut Serial) -> i32;
    static FRONT: sys::Symbol = sys::Symbol::of(LIBRARY, c"_SLPSGetFrontProcess");
    // SAFETY: the symbol is that function, which fills in the front process's serial
    // number and answers an error code.
    unsafe {
        let front = FRONT.function::<Front>()?;
        let mut serial = Serial::default();
        (front(&raw mut serial) == 0).then_some(serial)
    }
}

/// The pid of the process with this serial number.
pub fn pid_of(serial: Serial) -> Option<i32> {
    let mut pid = 0;
    // SAFETY: a serial number read, and an out-pointer for the pid, if there's a process.
    (unsafe { GetProcessPID(&raw const serial, &raw mut pid) } == 0).then_some(pid)
}

/// The serial number of the process `pid`.
fn serial_of(pid: i32) -> Option<Serial> {
    let mut serial = Serial::default();
    // SAFETY: an out-pointer for the serial number of a process, if there is one.
    (unsafe { GetProcessForPID(pid, &raw mut serial) } == 0).then_some(serial)
}

/// The app the window server has in front, by its pid. Any thread may ask.
pub fn front() -> Option<i32> {
    front_serial().and_then(pid_of)
}

/// Tells the app `pid`, and only it, that its window `window` is the key window: the pair
/// of records the window server itself sends an app as one of its windows takes the keys
/// (window managers send them to focus a window without raising it). The window server's
/// own key window, where the person's keys go, stays what it was. An app that draws its
/// own content, as Chromium and Electron do, takes clicks and keys only in a window it
/// believes is key.
pub fn make_key(pid: i32, window: u32) {
    type Post = unsafe extern "C" fn(*const Serial, *const u8) -> i32;
    static POST: sys::Symbol = sys::Symbol::of(LIBRARY, c"SLPSPostEventRecordTo");
    const SIZE: usize = 0xf8;
    // SAFETY: the symbol is that function.
    let Some(post) = (unsafe { POST.function::<Post>() }) else {
        return;
    };
    let Some(serial) = serial_of(pid) else {
        return;
    };
    let mut record = [0u8; SIZE];
    record[0x04] = 0xf8;
    record[0x3a] = 0x10;
    record[0x20..0x30].fill(0xff);
    record[0x3c..0x40].copy_from_slice(&window.to_le_bytes());
    // SAFETY: the symbol is that function; it reads a record of this size, laid out as the
    // window server's own (checked on this macOS in the lab), for a live process.
    unsafe {
        for phase in [0x01, 0x02] {
            record[0x08] = phase;
            post(&raw const serial, record.as_ptr());
        }
    }
}

/// The desktop the person is on.
pub fn active() -> Option<SpaceId> {
    let calls = calls()?;
    // SAFETY: reads the connection's active space.
    Some(unsafe { (calls.active_space)((calls.connection)()) })
}

/// The windows on `space`, front to back.
pub fn windows_on(space: SpaceId) -> Vec<u32> {
    let Some(calls) = calls() else {
        return Vec::new();
    };
    let spaces = array_of([space]);
    let (mut set, mut clear) = (0u64, 0u64);
    // SAFETY: an array of space numbers, two out-pointers, and a copy the caller owns.
    let list = ns::owned(unsafe {
        (calls.windows)(
            (calls.connection)(),
            0,
            ns::raw(&spaces),
            0x2,
            &raw mut set,
            &raw mut clear,
        )
    });
    ns::numbers(list)
}

/// The desktops `window` is on.
pub fn spaces_of(window: u32) -> Vec<SpaceId> {
    let Some(calls) = calls() else {
        return Vec::new();
    };
    let windows = array_of([u64::from(window)]);
    // SAFETY: an array of window numbers, the mask for every kind of space, a copy we own.
    let list =
        ns::owned(unsafe { (calls.spaces_of)((calls.connection)(), 0x7, ns::raw(&windows)) });
    ns::numbers(list)
}

/// The windows the window server keeps together with `window`, itself among them: a window
/// and the sheet on it.
pub fn together_with(window: u32) -> Vec<u32> {
    type Associated = unsafe extern "C" fn(i32, u32) -> Ref;
    static ASSOCIATED: sys::Symbol = sys::Symbol::of(LIBRARY, c"SLSCopyAssociatedWindows");
    // SAFETY: the symbol is that function, which takes a connection and a window number and
    // answers a list of numbers we own (checked on this macOS in the lab).
    let (Some(calls), Some(associated)) = (calls(), unsafe { ASSOCIATED.function::<Associated>() })
    else {
        return Vec::new();
    };
    // SAFETY: as above.
    let list = ns::owned(unsafe { associated((calls.connection)(), window) });
    ns::numbers(list)
}

/// Assigns the app `pid` to `space`: its windows go there now, and the ones it makes later
/// are made there.
pub fn assign(pid: i32, space: SpaceId) -> bool {
    let Some(calls) = calls() else {
        return false;
    };
    // SAFETY: a pid and a space number; the window server refuses what it won't do.
    unsafe { (calls.assign)((calls.connection)(), pid, space) == 0 }
}

/// Lets the app `pid` go from the desktop it was assigned to: its windows stay where they
/// are, and the ones it makes later open where the person is.
pub fn release(pid: i32) -> bool {
    assign(pid, 0)
}

/// Makes the app `pid` the front one at the window server itself, its windows left as they
/// lie. Asked of the window server and of nobody else, it's done within the call: Launch
/// Services' way (`activateWithOptions:`) took a second whenever the app in front was busy
/// coming forward, and that second is what took the person to the agent's desktop.
pub fn bring_front(pid: i32) -> bool {
    type SetFront = unsafe extern "C" fn(*const Serial, u32, u32) -> i32;
    static SET_FRONT: sys::Symbol = sys::Symbol::of(LIBRARY, c"_SLPSSetFrontProcessWithOptions");
    /// `kCPSNoWindows`: the process comes to the front, no window of it is raised.
    const NO_WINDOWS: u32 = 0x400;
    // SAFETY: the symbol is that function, which takes a process's serial number, a window
    // number (none) and a mode, as window managers call it.
    let Some(set) = (unsafe { SET_FRONT.function::<SetFront>() }) else {
        return false;
    };
    // SAFETY: as above, for a process that is there.
    serial_of(pid).is_some_and(|serial| unsafe { set(&raw const serial, 0, NO_WINDOWS) } == 0)
}

fn array_of(values: impl IntoIterator<Item = u64>) -> Obj {
    let numbers: Vec<Obj> = values
        .into_iter()
        .map(|value| ns::number(i64::try_from(value).unwrap_or(i64::MAX)))
        .collect();
    ns::array(&numbers)
}
