//! Opening and closing devices: a simulator of Koyo's set started (or made) in a tab of its
//! own, out of the person's sight unless they chose to see what agents open; and shut down
//! with its tab closed.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::device::{DeviceDone, DeviceOpenInput, DeviceOpenOutput, Which};

use super::looks::{screen, step};
use super::{claim, last_device, panes_of, remember, which};
use crate::control::Caller;
use crate::control::exec::{Exec, output, update_shell};
use crate::devices::{self, Chosen, Simulator, Standing};

/// How many simulators may run at once; Settings → Devices will let the person change it.
const RUNNING_AT_ONCE: usize = 2;
/// How long `device_open` waits for the device to stand ready before it answers that it is
/// still starting: inside the 60 s the tool promises.
const OPEN_WAIT: Duration = Duration::from_secs(50);
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// Xcode's tools failing, or a system missing: the person's to put right.
fn missing(why: impl std::fmt::Display) -> ControlError {
    super::needs_person(&format!(
        "{why} Koyo's simulators need Xcode 27 or later, opened once, and an iOS simulator system."
    ))
}

/// What runs now, by udid and name: the simulators the set has booted (an agent's own `simctl
/// boot`, a host still shutting one down) and those Koyo starts that haven't booted yet.
pub(super) type Running = Vec<(String, String)>;

fn starting(cx: &App) -> Running {
    let devices = devices::devices(cx);
    (devices.read(cx).links())
        .map(|link| link.read(cx))
        .filter(|link| link.running_or_starting())
        .map(|link| (link.udid.clone(), link.name.clone()))
        .collect()
}

/// Refused when `udid` (`None` for one to be made) would be one simulator too many.
pub(super) fn room_in(listed: &[Simulator], starting: &Running, udid: Option<&str>) -> Result<(), ControlError> {
    let mut running: Running = (listed.iter())
        .filter(|simulator| simulator.booted)
        .map(|simulator| (simulator.udid.clone(), simulator.name.clone()))
        .collect();
    for link in starting {
        if !running.iter().any(|(udid, _)| *udid == link.0) {
            running.push(link.clone());
        }
    }
    if udid.is_some_and(|udid| running.iter().any(|(running, _)| running == udid)) {
        return Ok(());
    }
    if running.len() < RUNNING_AT_ONCE {
        return Ok(());
    }
    let names: Vec<&str> = running.iter().map(|(_, name)| name.as_str()).collect();
    Err(ControlError::device_unavailable(format!(
        "{} simulators run already ({}), the most at once: close one with device_close, or ask the person.",
        names.len(),
        names.join(", ")
    ))
    .retryable(false))
}

/// The simulator `input` asks for: by its id, made from a model, or else the one this caller
/// used last, else the one booted last, else a new iPhone 17 Pro; refused past the limit before
/// anything is made.
fn choose(set: &Path, input: &DeviceOpenInput, last: Option<&str>, starting: &Running) -> Result<Simulator, ControlError> {
    use devices::simulators::{self, Unmade};
    let listed = simulators::list(set).map_err(missing)?;
    if let Some(model) = &input.model {
        room_in(&listed, starting, None)?;
        return simulators::make(set, model).map_err(|unmade| match unmade {
            Unmade::Model(why) => ControlError::invalid_input(why),
            Unmade::Missing(why) => missing(why),
        });
    }
    let known = |udid: &str| listed.iter().find(|simulator| simulator.udid == udid).cloned();
    let found = match (&input.device, last.and_then(known)) {
        (Some(udid), _) => known(udid)
            .ok_or_else(|| ControlError::not_found(format!("No simulator {udid} among Koyo's: device_list lists them.")))?,
        (None, Some(last)) => last,
        (None, None) => {
            let Some(found) = (listed.iter().find(|simulator| simulator.booted_before)).or(listed.first()) else {
                room_in(&listed, starting, None)?;
                return simulators::make_default(set).map_err(missing);
            };
            found.clone()
        }
    };
    room_in(&listed, starting, Some(&found.udid))?;
    Ok(found)
}

/// Opens the tab of `simulator`, started, for `caller`, whose device it is; its pane.
fn opened(caller: &Caller, simulator: &Simulator, cx: &mut App) -> Result<u64, ControlError> {
    let shows = crate::control::policy::shows(None, cx);
    let chosen = Chosen::from(simulator);
    let pane = update_shell(cx, |shell, _, cx| shell.device_pane(Some(chosen), true, shows, cx))?
        .ok_or_else(|| ControlError::internal("The device's tab couldn't be opened."))?;
    let devices = devices::devices(cx);
    // A simulator just made isn't in the list the launcher and the picker read yet.
    if devices.read(cx).simulator(&simulator.udid).is_none() {
        devices.update(cx, devices::Devices::refresh);
    }
    if let Some(link) = devices.read(cx).existing(&simulator.udid) {
        let _ = link.update(cx, |link, cx| link.agent_acts(caller.actor(), cx));
    }
    remember(caller, &simulator.udid, cx);
    Ok(pane)
}

/// How the device stands once opened: ready, or still starting at its step; or why it won't be.
fn standing(udid: &str, name: &str, pane: u64, cx: &App) -> Result<DeviceOpenOutput, ControlError> {
    let link = devices::devices(cx).read(cx).existing(udid);
    let link = link.as_ref().map(|link| link.read(cx));
    if let Some(error) = link.and_then(super::not_running) {
        return Err(error);
    }
    let out = |ready: bool, at: &str, screen_: Option<koyo_device_wire::Screen>| DeviceOpenOutput {
        device: udid.to_owned(),
        name: name.to_owned(),
        pane,
        ready,
        step: (!ready).then(|| at.to_owned()),
        screen: screen_.map(screen),
    };
    match link.map(devices::Link::standing) {
        Some(Standing::Ready(shown)) if link.is_some_and(devices::Link::ready) => Ok(out(true, "", Some(shown))),
        Some(Standing::Locked(_)) => Err(super::needs_person(&format!("{name} is locked: only the person can unlock it."))),
        Some(Standing::Starting(Some(at))) => Ok(out(false, step(at), None)),
        _ => Ok(out(false, "booting", None)),
    }
}

pub(crate) fn open(caller: &Caller, input: DeviceOpenInput, cx: &mut App) -> Result<Exec, ControlError> {
    // From the call's start: making a simulator is part of the minute the tool promises.
    let started = Instant::now();
    let set = devices::set_dir().ok_or_else(|| ControlError::internal("Koyo has no folder to keep its devices in."))?;
    let (last, starting) = (last_device(caller, cx), starting(cx));
    let chosen = cx.background_executor().spawn(async move { choose(&set, &input, last.as_deref(), &starting) });
    let caller = caller.clone();
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let simulator = chosen.await?;
        let pane = cx.update(|cx| opened(&caller, &simulator, cx))?;
        loop {
            let now = cx.update(|cx| standing(&simulator.udid, &simulator.name, pane, cx))?;
            if now.ready || started.elapsed() >= OPEN_WAIT {
                return output(&now);
            }
            cx.background_executor().timer(LOOK_EVERY).await;
        }
    })))
}

pub(crate) fn close(caller: &Caller, input: &Which, cx: &mut App) -> Result<Exec, ControlError> {
    let target = which(caller, input.device.as_deref(), input.pane, cx)?;
    // Refused while the person holds it, as an act is.
    claim(caller, &target, cx)?;
    // The last of its tabs closed ends its host; one with no tab is stopped as it is.
    let panes = panes_of(&target.udid, cx);
    let _ = update_shell(cx, |shell, _, cx| {
        for pane in panes {
            shell.close_pane_now(pane, cx);
        }
    });
    if target.link.read(cx).running_or_starting() {
        target.link.update(cx, devices::Link::stop);
    }
    Ok(Exec::Ready(output(&DeviceDone {
        device: target.udid,
        name: target.name,
    })))
}
