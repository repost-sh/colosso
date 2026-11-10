//! Making and removing a desktop, the one way a normal process can: Mission Control's own
//! buttons, pressed through accessibility (`docs/agent-desktop.md` 4). Mission Control shows
//! for a fraction of a second while it happens (86-109 ms to the new desktop, gone after
//! ~150 ms, measured on 27.2). Each display has a strip of its own; a desktop is found in
//! the strip of the display the window server lists it under.

use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use koyo_desk_wire::Rect;

use crate::ax::Element;
use crate::desktop;
use crate::ns;
use crate::skylight::{self, SpaceId};

/// How long Mission Control gets to show its strip, and a press to take.
const SHOWS_WITHIN: Duration = Duration::from_secs(3);
const TAKES_WITHIN: Duration = Duration::from_secs(2);

const APP: &str = "/System/Applications/Mission Control.app";

/// Opens or closes Mission Control, as its Dock icon does. Not activated: the person's front
/// app stays theirs.
fn toggle() {
    // SAFETY: the workspace's own launch call, with a configuration that activates nothing
    // and no completion handler.
    unsafe {
        let workspace: Retained<AnyObject> = msg_send![class!(NSWorkspace), sharedWorkspace];
        let url: Retained<AnyObject> = msg_send![class!(NSURL), fileURLWithPath: &*ns::string(APP)];
        let configuration: Retained<AnyObject> =
            msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
        let _: () = msg_send![&*configuration, setActivates: false];
        let _: () = msg_send![&*configuration, setAddsToRecentItems: false];
        let _: () = msg_send![
            &*workspace,
            openApplicationAtURL: &*url,
            configuration: &*configuration,
            completionHandler: ns::no_handler::<dyn Fn(*mut AnyObject, *mut AnyObject)>()
        ];
    }
}

/// The app whose accessibility tree holds Mission Control: `WindowManager` on macOS 27, the
/// Dock before.
fn owners() -> Vec<Element> {
    ["com.apple.WindowManager", "com.apple.dock"]
        .iter()
        .filter_map(|bundle| crate::apps::first_running(bundle))
        .filter_map(Element::app)
        .collect()
}

/// One display's strip of desktops, as Mission Control shows it.
struct Strip {
    /// Its buttons, a desktop each, in the window server's order.
    list: Element,
    add: Option<Element>,
    /// Where the strip is among the displays.
    frame: Option<Rect>,
}

fn strips() -> Vec<Strip> {
    owners()
        .iter()
        .flat_map(|owner| owner.all_with_identifier("mc.spaces"))
        .filter_map(|strip| {
            Some(Strip {
                list: strip.with_identifier("mc.spaces.list")?,
                add: strip.with_identifier("mc.spaces.add"),
                frame: strip.frame(),
            })
        })
        .collect()
}

/// The strip on the display at `bounds`; the only one when there's only one.
fn strip_on(strips: &[Strip], bounds: Option<Rect>) -> Option<&Strip> {
    if let ([only], 1) = (strips, desktop::displays().len()) {
        return Some(only);
    }
    let bounds = bounds?;
    (strips.iter()).find(|strip| {
        strip
            .frame
            .is_some_and(|frame| bounds.contains(frame.center()))
    })
}

/// Puts Mission Control up and finds its strips, every display's. Not when the person has
/// it up themselves: it's theirs, and toggling would close it.
fn show_strips() -> Result<Vec<Strip>, String> {
    if !strips().is_empty() {
        return Err("Mission Control is open: the person is using it".into());
    }
    toggle();
    let mut found = Vec::new();
    let displays = desktop::displays().len().max(1);
    ns::pump_until(SHOWS_WITHIN, || {
        found = strips();
        // Every display's strip, not the first that shows.
        found.len() >= displays
    });
    if found.is_empty() {
        put_away();
        return Err(
            "Mission Control's desktops strip didn't show: is Accessibility granted?".into(),
        );
    }
    Ok(found)
}

/// Puts Mission Control away again.
fn put_away() {
    toggle();
    ns::pump_until(SHOWS_WITHIN, || strips().is_empty());
}

/// Runs `act` with Mission Control up and its strips found, then puts it away.
fn with_strips<T>(act: impl FnOnce(&[Strip]) -> Result<T, String>) -> Result<T, String> {
    let found = show_strips()?;
    let done = act(&found);
    // Unasked, whatever the strips look like now: one that a desktop was just added to or
    // removed from is being rebuilt, and reads as gone for a moment.
    put_away();
    done
}

/// Adds a desktop, on the main display, and says which it is.
///
/// # Errors
/// When Mission Control's strip or its add button isn't found, or no desktop came.
pub fn add_desktop() -> Result<SpaceId, String> {
    let before = skylight::desktops();
    let main = (desktop::displays().into_iter())
        .find(|display| display.main)
        .map(|display| display.bounds);
    with_strips(|strips| {
        let strip = strip_on(strips, main)
            .or(strips.first())
            .ok_or("Mission Control shows no strip")?;
        let add = strip
            .add
            .as_ref()
            .ok_or("Mission Control has no add button")?;
        if !add.perform("AXPress") {
            return Err("Mission Control's add button didn't take the press".into());
        }
        let mut made = None;
        ns::pump_until(TAKES_WITHIN, || {
            made = skylight::desktops()
                .into_iter()
                .find(|space| !before.contains(space));
            made.is_some()
        });
        made.ok_or_else(|| "no desktop was added".to_owned())
    })
}

/// The desktop's display, its place among that display's desktops, and how many there are.
fn place(space: SpaceId) -> Option<(String, usize, usize)> {
    let spaces = skylight::spaces();
    let ours = spaces.iter().find(|listed| listed.id == space)?;
    let beside: Vec<SpaceId> = (spaces.iter())
        .filter(|listed| listed.display == ours.display)
        .map(|listed| listed.id)
        .collect();
    let index = beside.iter().position(|listed| *listed == space)?;
    Some((ours.display.clone(), index, beside.len()))
}

/// The button of the desktop `space` in its display's strip, read with Mission Control up,
/// right before it's pressed: the person may have moved or added a desktop since. A strip
/// that doesn't match the window server's list is still being rebuilt, or isn't this
/// display's: nothing is pressed by a place that may be another desktop's.
fn button(strips: &[Strip], space: SpaceId) -> Result<Element, String> {
    let (on, index, count) = place(space).ok_or("that desktop isn't there")?;
    let display = desktop::display_named(&on).map(|display| display.bounds);
    let strip = strip_on(strips, display).ok_or("the desktop's display shows no strip")?;
    let buttons = strip.list.children();
    if buttons.len() != count {
        return Err("Mission Control's strip doesn't match the desktops".into());
    }
    (buttons.into_iter().nth(index)).ok_or_else(|| "the desktop has no button".to_owned())
}

/// Puts the desktop `space` on the person's screen, as a click on it in Mission Control
/// does. Only ever on the person's word (`showing`).
///
/// # Errors
/// When the desktop isn't one of the person's, or its button isn't found, or the screen
/// stayed where it was.
pub fn go_to(space: SpaceId) -> Result<(), String> {
    if skylight::active() == Some(space) {
        return Ok(());
    }
    place(space).ok_or("that desktop isn't there")?;
    let found = show_strips()?;
    let pressed = button(&found, space).and_then(|button| {
        if button.perform("AXPress") {
            Ok(())
        } else {
            Err("the desktop's button didn't take the press".to_owned())
        }
    });
    let there =
        pressed.is_ok() && ns::pump_until(TAKES_WITHIN, || skylight::active() == Some(space));
    // A press on a desktop puts Mission Control away by itself; left up, it's put away.
    if !ns::pump_until(TAKES_WITHIN, || strips().is_empty()) {
        put_away();
    }
    pressed?;
    if there {
        Ok(())
    } else {
        Err("the screen stayed on the desktop it was on".into())
    }
}

/// Removes the desktop `space`; windows left on it go to the desktop beside it, so the
/// caller closes the agent's apps first.
///
/// # Errors
/// When the desktop isn't one of the person's, or its button isn't found, or it stayed.
pub fn remove_desktop(space: SpaceId) -> Result<(), String> {
    place(space).ok_or("that desktop isn't there")?;
    with_strips(|strips| {
        let button = button(strips, space)?;
        if !button.perform("AXRemoveDesktop") {
            return Err("the desktop's button didn't take the removal".into());
        }
        let gone = ns::pump_until(TAKES_WITHIN, || !skylight::desktops().contains(&space));
        if gone {
            Ok(())
        } else {
            Err("the desktop stayed".into())
        }
    })
}
