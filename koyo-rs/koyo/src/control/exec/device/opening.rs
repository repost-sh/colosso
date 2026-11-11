//! Opening and closing devices: a simulator of Koyo's set started (or made) in a tab of its
//! own, out of the person's sight unless they chose to see what agents open; and shut down
//! with its tab closed.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::device::{DeviceDone, DeviceOpenInput, DeviceOpenOutput, Which};
use koyo_device_wire::{Missing, State as Said};

use super::looks::{screen, step};
use super::{claim, last_device, panes_of, remember, which};
use crate::control::Caller;
use crate::control::exec::{Exec, output, update_shell};
use crate::devices::{self, Chosen, Simulator, State};

/// How many simulators may run at once; Settings → Devices will let the person change it.
const RUNNING_AT_ONCE: usize = 2;
/// How long `device_open` waits for the device to stand ready before it answers that it is
/// still starting: inside the 60 s the tool promises.
const OPEN_WAIT: Duration = Duration::from_secs(50);
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// The simulator `input` asks for: by its id, made from a model, or else the one this caller
/// used last, else the one booted last, else a new iPhone 17 Pro.
fn choose(set: &Path, input: &DeviceOpenInput, last: Option<&str>) -> Result<Simulator, ControlError> {
    use devices::simulators;
    if let Some(model) = &input.model {
        return simulators::make(set, model).map_err(ControlError::invalid_input);
    }
    let listed = simulators::list(set).map_err(ControlError::internal)?;
    if let Some(udid) = &input.device {
        return (listed.into_iter().find(|simulator| &simulator.udid == udid))
            .ok_or_else(|| ControlError::not_found(format!("No simulator {udid} among Koyo's: device_list lists them.")));
    }
    let known = |udid: &str| listed.iter().find(|simulator| simulator.udid == udid).cloned();
    if let Some(found) = last.and_then(known) {
        return Ok(found);
    }
    if let Some(found) = (listed.iter()).find(|simulator| simulator.booted_before) {
        return Ok(found.clone());
    }
    match listed.first() {
        Some(first) => Ok(first.clone()),
        None => simulators::make_default(set).map_err(ControlError::internal),
    }
}

/// Refused when `udid` would be one simulator too many.
fn room_for(udid: &str, cx: &App) -> Result<(), ControlError> {
    let devices = devices::devices(cx);
    let running: Vec<String> = (devices.read(cx).links())
        .filter(|link| link.read(cx).running_or_starting() && link.read(cx).udid != udid)
        .map(|link| link.read(cx).name.clone())
        .collect();
    if running.len() < RUNNING_AT_ONCE {
        return Ok(());
    }
    Err(ControlError::device_unavailable(format!(
        "{} simulators run already ({}), the most at once: close one with device_close, or ask the person.",
        running.len(),
        running.join(", ")
    ))
    .retryable(false))
}

/// Opens the tab of `simulator`, started, for `caller`, whose device it is; its pane.
fn opened(caller: &Caller, simulator: &Simulator, cx: &mut App) -> Result<u64, ControlError> {
    room_for(&simulator.udid, cx)?;
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
    let (state, said) = link.map_or((State::Off, None), |link| (link.state.clone(), link.said.clone()));
    let out = |ready: bool, at: &str| DeviceOpenOutput {
        device: udid.to_owned(),
        name: name.to_owned(),
        pane,
        ready,
        step: (!ready).then(|| at.to_owned()),
        screen: link.and_then(devices::Link::screen).map(screen),
    };
    match (state, said) {
        (State::Failed(why), _) => Err(ControlError::internal(why)),
        (State::Off, _) => Err(ControlError::device_unavailable(format!(
            "{name} was stopped before it stood ready: device_open opens it again."
        ))
        .retryable(false)),
        (_, Some(Said::Unavailable { code, why })) => Err(match code {
            Missing::Xcode | Missing::System | Missing::Simulator => ControlError::device_needs_person(format!(
                "{why} Settings → Devices walks the person through it."
            )),
            _ => ControlError::internal(why),
        }),
        (_, Some(Said::Gone { why })) => Err(ControlError::device_unavailable(format!("{name} went away: {why}."))),
        (_, Some(Said::Locked { .. })) => Err(ControlError::device_needs_person(format!(
            "{name} is locked: only the person can unlock it."
        ))),
        (State::Running, Some(Said::Ready { .. })) => Ok(out(true, "")),
        (_, Some(Said::Starting { step: at })) => Ok(out(false, step(at))),
        _ => Ok(out(false, "booting")),
    }
}

pub(crate) fn open(caller: &Caller, input: DeviceOpenInput, cx: &mut App) -> Result<Exec, ControlError> {
    // From the call's start: making a simulator is part of the minute the tool promises.
    let started = Instant::now();
    let set = devices::set_dir().ok_or_else(|| ControlError::internal("Koyo has no folder to keep its devices in."))?;
    // Refused before anything is made or started: a new simulator, or one not running, with no
    // room for it.
    match (&input.model, &input.device) {
        (Some(_), _) => room_for("", cx)?,
        (None, Some(udid)) => room_for(udid, cx)?,
        (None, None) => {}
    }
    let last = last_device(caller, cx);
    let chosen = cx.background_executor().spawn(async move { choose(&set, &input, last.as_deref()) });
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
