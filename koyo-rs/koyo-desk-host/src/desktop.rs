//! The agent desktop's life (`docs/agent-desktop.md` 4): made when the host starts, kept, and
//! removed when Koyo quits. What was made and what was started on it is recorded, so a run
//! that crashed is cleaned up at the next start: its apps ended, its desktop removed, and
//! nothing else.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use koyo_desk_wire::{Point, Rect, Size};

use crate::mission;
use crate::ns;
use crate::skylight::{self, SpaceId};
use crate::sys;

pub struct Desktop {
    pub space: SpaceId,
    /// The display it's on: its place among the displays, in points, and its pixels a point.
    pub display: Rect,
    pub scale: f64,
}

/// A display: its name as the window server has it, its place, its pixels a point.
pub struct Display {
    pub name: String,
    pub bounds: Rect,
    pub scale: f64,
    pub main: bool,
}

/// The displays that are on.
pub fn displays() -> Vec<Display> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: room for 16 display numbers and a count; plain reads of each display after.
    unsafe {
        if sys::CGGetActiveDisplayList(16, ids.as_mut_ptr(), &raw mut count) != 0 {
            return Vec::new();
        }
        let main = sys::CGMainDisplayID();
        ids.iter()
            .take(count as usize)
            .map(|&id| {
                let bounds = sys::CGDisplayBounds(id);
                let name = ns::owned(sys::CGDisplayCreateUUIDFromDisplayID(id))
                    .and_then(|uuid| {
                        ns::owned(sys::CFUUIDCreateString(std::ptr::null(), ns::raw(&uuid)))
                    })
                    .and_then(|name| ns::text(&name))
                    .unwrap_or_default();
                let scale = ns::owned(sys::CGDisplayCopyDisplayMode(id)).map_or(1.0, |mode| {
                    let points = sys::CGDisplayModeGetWidth(ns::raw(&mode)).max(1);
                    sys::CGDisplayModeGetPixelWidth(ns::raw(&mode)) as f64 / points as f64
                });
                Display {
                    name,
                    bounds: Rect {
                        x: bounds.origin.x,
                        y: bounds.origin.y,
                        width: bounds.size.width,
                        height: bounds.size.height,
                    },
                    scale: scale.max(1.0),
                    main: id == main,
                }
            })
            .collect()
    }
}

/// The display the window server lists desktops under as `name`; the main one for `Main`,
/// which is what it says when all displays share one set of desktops.
pub fn display_named(name: &str) -> Option<Display> {
    let mut displays = displays();
    let at = (displays.iter())
        .position(|display| display.name.eq_ignore_ascii_case(name))
        .or_else(|| displays.iter().position(|display| display.main))?;
    Some(displays.swap_remove(at))
}

/// A process the host started or took in, by its number and when it started: together they
/// name one process for good, where a number alone is used again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Started {
    pub pid: i32,
    pub at: u64,
    /// The person's own, lent (`lending`): given back, never ended.
    pub lent: bool,
    /// The desktop a lent app came from.
    pub home: Option<SpaceId>,
}

/// When the process `pid` started, in seconds since 1970; `None` when there's none.
pub fn started_at(pid: i32) -> Option<u64> {
    // SAFETY: a zeroed record of the size the call is told, filled by the kernel.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
        let read = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size);
        (read == size).then_some(info.pbi_start_tvsec)
    }
}

fn desktop_record(state: &Path) -> PathBuf {
    state.join("desktop")
}

fn apps_record(state: &Path) -> PathBuf {
    state.join("apps")
}

/// Writes down the apps the host has started, for a later run to end if this one can't, and
/// the person's that are lent, for it to give back.
pub fn record_apps(state: &Path, apps: &[Started]) {
    let _ = std::fs::write(apps_record(state), apps_text(apps));
}

fn apps_text(apps: &[Started]) -> String {
    let mut text = String::new();
    for app in apps {
        let _ = write!(text, "{} {}", app.pid, app.at);
        if app.lent {
            let _ = write!(text, " lent {}", app.home.unwrap_or_default());
        }
        text.push('\n');
    }
    text
}

fn parse_apps(text: &str) -> Vec<Started> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(' ');
            Some(Started {
                pid: parts.next()?.parse().ok()?,
                at: parts.next()?.parse().ok()?,
                lent: parts.next() == Some("lent"),
                home: (parts.next().and_then(|home| home.parse().ok())).filter(|home| *home != 0),
            })
        })
        .collect()
}

/// The person's desktops: every one but the agent's, and no app's full screen.
fn persons(agents: Option<SpaceId>) -> Vec<SpaceId> {
    (skylight::spaces().iter())
        .filter(|space| !space.full_screen && Some(space.id) != agents)
        .map(|space| space.id)
        .collect()
}

/// The desktop of the person's to go back to: `wanted`, when it's still one of theirs, else
/// the one they're on, else their first.
pub fn back_to(wanted: Option<SpaceId>, agents: Option<SpaceId>) -> Option<SpaceId> {
    let theirs = persons(agents);
    let among = |space: &SpaceId| theirs.contains(space);
    (wanted.filter(among))
        .or_else(|| skylight::active().filter(among))
        .or(theirs.first().copied())
}

/// Where an app of the person's shows its windows.
pub struct Shown {
    /// The desktop of theirs that holds most of them (the first of those, as Mission
    /// Control orders them): where the app goes back to.
    pub home: Option<SpaceId>,
    /// One is in full screen: a space of its own, which an assignment doesn't move.
    pub full_screen: bool,
}

/// Where the app `pid` shows its windows, the agent's desktop left out.
pub fn shown(pid: i32, agents: Option<SpaceId>) -> Shown {
    let (mut most, mut full_screen) = (None, false);
    for space in skylight::spaces() {
        let count = crate::windows::on(space.id, &[pid]).len();
        if count == 0 || Some(space.id) == agents {
            continue;
        }
        if space.full_screen {
            full_screen = true;
        } else if most.is_none_or(|(_, held)| count > held) {
            most = Some((space.id, count));
        }
    }
    Shown {
        home: most.map(|(space, _)| space),
        full_screen,
    }
}

/// Gives the app `pid` back to the person: its windows go to the desktop it came from
/// (`home`), or to the one the person is on when that one is gone; and what it makes from
/// now on is made where they are, as before it was lent.
pub fn hand_back(pid: i32, home: Option<SpaceId>, agents: Option<SpaceId>) {
    if let Some(to) = back_to(home, agents) {
        skylight::assign(pid, to);
    }
    skylight::release(pid);
}

/// Once, as the host starts and before it makes its desktop (never later: by then the
/// record is of the host's own running apps). Clears what a run that crashed left behind:
/// the person's apps it had lent go back to them; the apps it started are ended, when they
/// are still the same processes; and then its desktop is removed, when it's still there
/// under the same name and no app keeps a window on it.
pub fn remove_leftovers(state: &Path) {
    let record = desktop_record(state);
    let desktop = std::fs::read_to_string(&record).unwrap_or_default();
    let mut parts = desktop.split_whitespace();
    let recorded = (parts.next().and_then(|id| id.parse::<SpaceId>().ok())).zip(parts.next());

    let apps = std::fs::read_to_string(apps_record(state)).unwrap_or_default();
    let (lent, ours): (Vec<Started>, Vec<Started>) = parse_apps(&apps)
        .into_iter()
        .filter(|app| started_at(app.pid) == Some(app.at))
        .partition(|app| app.lent);
    // The person's own go back to them as they are, before anything is ended.
    let agents = recorded.map(|(space, _)| space);
    for app in &lent {
        hand_back(app.pid, app.home, agents);
    }
    if let Some(space) = agents
        && !lent.is_empty()
    {
        // Their windows take a moment to leave the desktop, which is removed only empty.
        ns::pump_until(Duration::from_secs(1), || {
            !crate::windows::holds_app_windows(space)
        });
    }
    for (signal, wait) in [(libc::SIGTERM, 2000), (libc::SIGKILL, 1000)] {
        let alive = |app: &&Started| started_at(app.pid) == Some(app.at);
        for app in ours.iter().filter(alive) {
            // SAFETY: a signal to one process, checked just above to be the one recorded.
            unsafe { libc::kill(app.pid, signal) };
        }
        ns::pump_until(Duration::from_millis(wait), || {
            !ours.iter().any(|app| alive(&app))
        });
    }
    let _ = std::fs::remove_file(apps_record(state));

    if let Some((space, name)) = recorded {
        let still = skylight::spaces()
            .iter()
            .any(|listed| listed.id == space && listed.name == name);
        if still && !crate::windows::holds_app_windows(space) {
            let _ = mission::remove_desktop(space);
        }
    }
    let _ = std::fs::remove_file(record);
}

impl Desktop {
    /// Makes the agent desktop.
    ///
    /// # Errors
    /// Says what is missing: a grant, the window server's calls, Mission Control's button.
    pub fn make(state: &Path) -> Result<Desktop, String> {
        if !skylight::available() {
            return Err(
                "this macOS doesn't have the window server's calls the agent desktop needs".into(),
            );
        }
        // SAFETY: a plain read of this process's accessibility grant.
        if unsafe { sys::AXIsProcessTrusted() } == 0 {
            return Err("Accessibility isn't granted to the agent desktop's host".into());
        }
        let _ = std::fs::create_dir_all(state);
        let space = mission::add_desktop()?;
        let listed = skylight::spaces()
            .into_iter()
            .find(|listed| listed.id == space);
        let (name, on) = listed
            .map(|listed| (listed.name, listed.display))
            .unwrap_or_default();
        let _ = std::fs::write(desktop_record(state), format!("{space} {name}\n"));
        let display = display_named(&on).ok_or("no display is on")?;
        Ok(Desktop {
            space,
            display: display.bounds,
            scale: display.scale,
        })
    }

    pub fn size(&self) -> Size {
        Size {
            width: self.display.width,
            height: self.display.height,
        }
    }

    /// A point of the desktop, counted from its top left, as a point among the displays.
    pub fn global(&self, point: Point) -> Point {
        Point {
            x: point.x + self.display.x,
            y: point.y + self.display.y,
        }
    }

    /// A rectangle among the displays as the desktop counts it, from its top left.
    pub fn local(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x - self.display.x,
            y: rect.y - self.display.y,
            ..rect
        }
    }

    /// Removes the desktop, and the records of it in `state`. The agent's apps are closed
    /// first by the caller: windows left on a desktop that goes move to the one beside it.
    pub fn remove(self, state: &Path) {
        if mission::remove_desktop(self.space).is_ok() {
            let _ = std::fs::remove_file(desktop_record(state));
            let _ = std::fs::remove_file(apps_record(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_record_reads_back() {
        let apps = [
            Started {
                pid: 412,
                at: 1_794_000_000,
                lent: false,
                home: None,
            },
            Started {
                pid: 9001,
                at: 1_794_000_321,
                lent: true,
                home: Some(4),
            },
            Started {
                pid: 77,
                at: 1_794_000_400,
                lent: true,
                home: None,
            },
        ];
        let text = apps_text(&apps);
        assert_eq!(
            text,
            "412 1794000000\n9001 1794000321 lent 4\n77 1794000400 lent 0\n"
        );
        assert_eq!(parse_apps(&text), apps);
        assert_eq!(parse_apps("junk\n12 x\n"), []);
    }

    #[test]
    fn this_process_has_a_start_time_and_none_has_no_process() {
        let me = i32::try_from(std::process::id()).unwrap();
        assert!(started_at(me).is_some_and(|at| at > 1_600_000_000));
        assert_eq!(started_at(i32::MAX - 7), None);
    }
}
