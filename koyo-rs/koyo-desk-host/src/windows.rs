//! The windows on the agent desktop (`docs/agent-desktop.md` 6): which the window server
//! lists there, front to back, and what each is.

use std::collections::{HashMap, HashSet};

use objc2_core_foundation::CGRect;

use koyo_desk_wire::Rect;
use koyo_desk_wire::link::Window;

use crate::ns;
use crate::skylight::{self, SpaceId};
use crate::sys;

/// Smaller than this a window is a helper nobody sees.
const SMALLEST: f64 = 24.0;

/// The layers the desktop's own windows are on, far below every app's: its wallpaper, and
/// what lies on that.
const DESKTOP_LAYERS: i32 = -2_000_000_000;

/// The windows on `space` that belong to one of `pids`, front to back.
pub fn on(space: SpaceId, pids: &[i32]) -> Vec<Window> {
    ours(all_on(space), pids)
}

/// [`on`], and the desktop's own picture: the wallpaper window macOS keeps on `space`,
/// covering its `display`. That is shown behind the agent's windows, and nothing is ever
/// done to it. Both from one reading of what the window server lists there.
pub fn with_backdrop(space: SpaceId, pids: &[i32], display: Rect) -> (Vec<Window>, Option<Window>) {
    let all = all_on(space);
    let backdrop = backdrop_among(&all, display).cloned();
    (ours(all, pids), backdrop)
}

/// Those of `all` that belong to one of `pids` and are large enough to be a window.
fn ours(mut all: Vec<Window>, pids: &[i32]) -> Vec<Window> {
    all.retain(|window| pids.contains(&window.pid) && large(window));
    all
}

fn large(window: &Window) -> bool {
    window.frame.width >= SMALLEST && window.frame.height >= SMALLEST
}

fn backdrop_among(all: &[Window], display: Rect) -> Option<&Window> {
    (all.iter())
        .filter(|window| window.layer < DESKTOP_LAYERS && same(window.frame, display))
        .min_by_key(|window| window.layer)
}

/// Whether the app `pid` shows a window on a desktop other than `space`.
pub fn elsewhere(pid: i32, space: SpaceId) -> bool {
    (skylight::spaces().iter()).any(|other| other.id != space && !on(other.id, &[pid]).is_empty())
}

/// The app of the person's to have in front on `space`: `had`, when it has a window there,
/// else the one whose window is frontmost there; the apps `except` left out. Read from one
/// listing of the desktop: this is asked while the person waits for their front app.
pub fn persons_front(space: SpaceId, had: Option<i32>, except: &[i32]) -> Option<i32> {
    let all = all_on(space);
    let mut theirs = (all.iter())
        .filter(|window| window.layer == 0 && large(window) && !except.contains(&window.pid))
        .map(|window| window.pid);
    let frontmost = theirs.next()?;
    let has = |had: &i32| *had == frontmost || theirs.any(|pid| pid == *had);
    Some(had.filter(has).unwrap_or(frontmost))
}

/// Whether an app keeps a window on `space` alone. Every desktop also holds the system's own
/// (the menu bar, the wallpaper) and the windows apps show on all desktops: those don't count.
pub fn holds_app_windows(space: SpaceId) -> bool {
    all_on(space)
        .iter()
        .any(|window| window.layer == 0 && skylight::spaces_of(window.id) == [space])
}

/// Every window the window server lists on `space`, front to back.
fn all_on(space: SpaceId) -> Vec<Window> {
    let ids = skylight::windows_on(space);
    let mut windows: Vec<Window> = described(&ids).iter().filter_map(window).collect();
    // The description comes in no promised order: put it back in the window server's.
    windows.sort_by_key(|window| ids.iter().position(|id| *id == window.id));
    windows
}

/// What the window server says of each of `ids`, in no promised order.
fn described(ids: &[u32]) -> Vec<ns::Obj> {
    if ids.is_empty() {
        return Vec::new();
    }
    // The window list takes the numbers themselves, not number objects.
    let values: Vec<*const std::ffi::c_void> = ids
        .iter()
        .map(|id| std::ptr::without_provenance(*id as usize))
        .collect();
    // SAFETY: an array of plain values (no callbacks), and a description we own.
    let described = unsafe {
        let array = sys::CFArrayCreate(
            std::ptr::null(),
            values.as_ptr(),
            values.len().cast_signed(),
            std::ptr::null(),
        );
        let described = sys::CGWindowListCreateDescriptionFromArray(array);
        if !array.is_null() {
            sys::CFRelease(array);
        }
        ns::owned(described)
    };
    described
        .map(|described| ns::items(&described))
        .unwrap_or_default()
}

/// The number of every window in the session, on whichever desktop.
fn every() -> HashSet<u32> {
    /// `kCGWindowListOptionAll`.
    const ALL: u32 = 0;
    // SAFETY: a list we own, of plain numbers (not objects), read below its count.
    unsafe {
        let list = sys::CGWindowListCreate(ALL, 0);
        if list.is_null() {
            return HashSet::new();
        }
        let ids = (0..sys::CFArrayGetCount(list))
            .filter_map(|index| u32::try_from(sys::CFArrayGetValueAtIndex(list, index).addr()).ok())
            .collect();
        sys::CFRelease(list);
        ids
    }
}

/// The process macOS holds responsible for `pid`: an app for itself, and for the services
/// macOS runs on its behalf.
fn responsible_for(pid: i32) -> Option<i32> {
    type Responsible = unsafe extern "C" fn(i32) -> i32;
    static RESPONSIBLE: sys::Symbol =
        sys::Symbol::loaded(c"responsibility_get_pid_responsible_for_pid");
    // SAFETY: the symbol is that function of the system library, which takes a process's
    // number and answers another's (checked on this macOS in the lab).
    let found = unsafe { RESPONSIBLE.function::<Responsible>()?(pid) };
    (found > 0).then_some(found)
}

/// A window another process keeps for one of the agent's and draws its content in: an Open
/// or Save panel's, which macOS runs in a service of its own for an app in a sandbox. What
/// is clicked and typed in the agent's window is that process's to take: events sent to the
/// app itself never reach the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hosted {
    /// The agent's window, as listed.
    pub of: u32,
    pub pid: i32,
    pub window: u32,
}

/// Where the events for the agent's window `id` go in the app's place, if somewhere.
pub fn through(hosted: &[Hosted], id: u32) -> Option<(i32, u32)> {
    (hosted.iter())
        .find(|hosted| hosted.of == id)
        .map(|hosted| (hosted.pid, hosted.window))
}

/// The agent's window that `id` is, or is kept for.
pub fn listed_for(hosted: &[Hosted], id: u32) -> u32 {
    (hosted.iter())
        .find(|hosted| hosted.window == id)
        .map_or(id, |hosted| hosted.of)
}

/// The session's windows as they were looked at, so each is looked at once.
#[derive(Default)]
pub struct Helpers {
    /// The windows of processes that work for one of the agent's apps: the window and the
    /// app.
    theirs: Vec<(u32, i32)>,
    /// Every other window seen.
    others: HashSet<u32>,
}

impl Helpers {
    /// The windows that processes working for the agent's apps (`pids`) keep exactly over
    /// one of that app's `listed` windows.
    pub fn hosted(&mut self, listed: &[Window], pids: &[i32]) -> Vec<Hosted> {
        // No app of the agent's, no window drawn for one: every window in the session
        // needn't be listed to find that out.
        if pids.is_empty() {
            return Vec::new();
        }
        let ids = every();
        self.theirs.retain(|(id, ..)| ids.contains(id));
        self.others.retain(|id| ids.contains(id));
        let new: Vec<u32> = (ids.into_iter())
            .filter(|id| {
                !self.others.contains(id) && !self.theirs.iter().any(|(known, ..)| known == id)
            })
            .collect();
        let mut apps: HashMap<i32, Option<i32>> = HashMap::new();
        for entry in described(&new) {
            let number = |key: &str| {
                let value = ns::entry(&entry, key).and_then(|value| ns::integer(&value))?;
                i32::try_from(value).ok()
            };
            let (Some(id), Some(pid)) = (number("kCGWindowNumber"), number("kCGWindowOwnerPID"))
            else {
                continue;
            };
            let app = *apps.entry(pid).or_insert_with(|| responsible_for(pid));
            if let (Some(app), false, Some(0), Ok(id)) = (
                app.filter(|app| pids.contains(app)),
                pids.contains(&pid),
                number("kCGWindowLayer"),
                u32::try_from(id),
            ) {
                self.theirs.push((id, app));
            }
        }
        // What isn't a helper's now is never looked at again.
        (self.others).extend(
            new.iter()
                .filter(|id| !self.theirs.iter().any(|(known, ..)| known == *id)),
        );
        if self.theirs.is_empty() {
            return Vec::new();
        }
        let kept: Vec<u32> = self.theirs.iter().map(|(id, ..)| *id).collect();
        (described(&kept).iter().filter_map(window))
            .filter_map(|part| {
                let (_, app) = self.theirs.iter().find(|(id, _)| *id == part.id)?;
                let of = (listed.iter())
                    .find(|window| window.pid == *app && same(window.frame, part.frame))?;
                Some(Hosted {
                    of: of.id,
                    pid: part.pid,
                    window: part.id,
                })
            })
            .collect()
    }
}

/// Whether two frames are one place, to the point.
fn same(one: Rect, other: Rect) -> bool {
    let near = |a: f64, b: f64| (a - b).abs() <= 1.0;
    near(one.x, other.x)
        && near(one.y, other.y)
        && near(one.width, other.width)
        && near(one.height, other.height)
}

fn window(entry: &ns::Obj) -> Option<Window> {
    let number = |key: &str| ns::entry(entry, key).and_then(|value| ns::integer(&value));
    let text = |key: &str| ns::entry(entry, key).and_then(|value| ns::text(&value));
    let alpha = ns::entry(entry, "kCGWindowAlpha").and_then(|value| ns::float(&value));
    if alpha == Some(0.0) {
        return None;
    }
    let bounds = ns::entry(entry, "kCGWindowBounds")?;
    let mut rect = CGRect::default();
    // SAFETY: a dictionary the window list made for a rectangle, and an out-pointer.
    if !unsafe { sys::CGRectMakeWithDictionaryRepresentation(ns::raw(&bounds), &raw mut rect) } {
        return None;
    }
    Some(Window {
        id: u32::try_from(number("kCGWindowNumber")?).ok()?,
        pid: i32::try_from(number("kCGWindowOwnerPID")?).ok()?,
        app: text("kCGWindowOwnerName").unwrap_or_default(),
        title: text("kCGWindowName").unwrap_or_default(),
        frame: Rect {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        },
        layer: i32::try_from(number("kCGWindowLayer").unwrap_or(0)).unwrap_or(0),
    })
}

/// The agent's windows on its desktop, front to back; none without a desktop.
pub fn on_desktop(host: &crate::host::Host) -> Vec<Window> {
    (host.desktop.as_ref())
        .map(|desktop| on(desktop.space, &host.pids()))
        .unwrap_or_default()
}

/// The frontmost window under `point`, among `windows` (front to back).
pub fn under(windows: &[Window], point: koyo_desk_wire::Point) -> Option<&Window> {
    windows.iter().find(|window| window.frame.contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktops_picture_is_its_wallpaper_and_nothing_else() {
        let window = |id, layer, (width, height): (f64, f64)| Window {
            id,
            pid: 400,
            app: "WindowManager".into(),
            title: String::new(),
            frame: Rect {
                x: 0.,
                y: 0.,
                width,
                height,
            },
            layer,
        };
        let display = window(0, 0, (1800., 1169.)).frame;
        let all = vec![
            window(1, 24, (1800., 39.)),
            window(2, 0, (1800., 1169.)),
            window(3, -2_147_483_602, (1800., 99.)),
            window(4, -2_147_483_624, (1800., 1169.)),
        ];
        assert_eq!(backdrop_among(&all, display).map(|found| found.id), Some(4));
        // A window as large as the display isn't the wallpaper for that: an app's can be.
        let apps = vec![window(2, 0, (1800., 1169.))];
        assert_eq!(backdrop_among(&apps, display), None);
    }

    #[test]
    fn events_for_a_window_another_process_draws_go_to_that_process() {
        let hosted = [Hosted {
            of: 10,
            pid: 900,
            window: 11,
        }];
        assert_eq!(through(&hosted, 10), Some((900, 11)));
        assert_eq!(through(&hosted, 12), None);
        // What accessibility names by the other process's window is the agent's window.
        assert_eq!(listed_for(&hosted, 11), 10);
        assert_eq!(listed_for(&hosted, 12), 12);
    }
}
