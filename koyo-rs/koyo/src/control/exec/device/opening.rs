//! Opening and closing devices: a simulator of Koyo's set started (or made) in a tab of its
//! own, or a real device plugged in and lent to the agent, out of the person's sight unless they
//! chose to see what agents open; and shut down or let go with its tab closed.

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

/// What Koyo holds by udid and name, and how many may run at once.
pub(super) struct Room {
    pub starting: Vec<(String, String)>,
    pub at_once: usize,
}

/// Refused when `udid` (`None` for one to be made) would be one simulator too many.
pub(super) fn room_in(listed: &[Simulator], room: &Room, udid: Option<&str>) -> Result<(), ControlError> {
    devices::past_limit(&devices::running(listed, &room.starting), udid, room.at_once).map_or(Ok(()), |names| Err(too_many(&names)))
}

/// One simulator too many, `names` running.
fn too_many(names: &[String]) -> ControlError {
    ControlError::device_unavailable(format!(
        "{}, the most the person allows at once (Settings → Devices): close one with device_close, or ask the person.",
        devices::running_already(names)
    ))
    .retryable(false)
}

/// The simulator `input` asks for: by its id, made from a model, or else the one this caller
/// used last, else the one booted last, else a new iPhone, the newest; refused past the limit before
/// anything is made.
fn choose(set: &Path, input: &DeviceOpenInput, last: Option<&str>, room: &Room) -> Result<Simulator, ControlError> {
    use devices::simulators::{self, Unmade};
    let listed = simulators::list(set).map_err(missing)?;
    let make = |model: Option<&str>| {
        room_in(&listed, room, None)?;
        simulators::make(set, model).map_err(|unmade| match unmade {
            Unmade::Model(why) => ControlError::invalid_input(why),
            Unmade::Missing(why) => missing(why),
        })
    };
    if let Some(model) = &input.model {
        return make(Some(model));
    }
    let known = |udid: &str| listed.iter().find(|simulator| simulator.udid == udid).cloned();
    let found = match (&input.device, last.and_then(known)) {
        (Some(udid), _) => known(udid)
            .ok_or_else(|| ControlError::not_found(format!("No simulator {udid} among Koyo's: device_list lists them.")))?,
        (None, Some(last)) => last,
        (None, None) => match (listed.iter().find(|simulator| simulator.booted_before)).or(listed.first()) {
            Some(found) => found.clone(),
            None => return make(None),
        },
    };
    room_in(&listed, room, Some(&found.udid))?;
    Ok(found)
}

/// Opens the tab of `chosen`, started, for `caller`, whose device it is (a real one lent to
/// it); its pane.
fn opened(caller: &Caller, chosen: Chosen, cx: &mut App) -> Result<u64, ControlError> {
    let shows = crate::control::policy::shows(None, cx);
    let udid = chosen.udid.clone();
    let simulator = !chosen.phone;
    // Asked again as it starts: another call may have started one since this one looked.
    if simulator {
        let running = devices::devices(cx).read(cx).running_now(cx);
        if let Some(names) = devices::past_limit(&running, Some(&udid), devices::at_once(cx)) {
            return Err(too_many(&names));
        }
    }
    let pane = update_shell(cx, |shell, _, cx| shell.device_pane(Some(chosen), shows, cx))?
        .ok_or_else(|| ControlError::internal("The device's tab couldn't be opened."))?;
    let devices = devices::devices(cx);
    // A simulator just made isn't in the list the launcher and the picker read yet.
    if simulator && devices.read(cx).simulator(&udid).is_none() {
        devices.update(cx, devices::Devices::refresh);
    }
    if let Some(link) = devices.read(cx).existing(&udid) {
        link.update(cx, devices::Link::start);
        let _ = link.update(cx, |link, cx| link.agent_acts(caller.actor(), cx));
    }
    remember(caller, &udid, cx);
    Ok(pane)
}

/// Waits for the device opened in `pane` to stand ready, inside the minute the tool promises
/// from `started`; says how it stands then.
async fn until_ready(udid: String, name: String, pane: u64, started: Instant, cx: &mut AsyncApp) -> Result<serde_json::Value, ControlError> {
    loop {
        let now = cx.update(|cx| standing(&udid, &name, pane, cx))?;
        if now.ready || started.elapsed() >= OPEN_WAIT {
            return output(&now);
        }
        cx.background_executor().timer(LOOK_EVERY).await;
    }
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
    if let Some(udid) = input.device.as_deref() {
        let unplugged = devices::devices(cx).read(cx).plugged_device(udid).is_none();
        if let Some((name, _)) = super::lend::real(udid, cx).filter(|_| unplugged) {
            return Err(ControlError::device_unavailable(format!("{name} isn't plugged in: plug it in with a cable.")).retryable(false));
        }
        super::lend::usable(caller, udid, cx)?;
        // A real device plugged in, lent to the caller: no simulator to choose or make.
        if let Some(plugged) = devices::devices(cx).read(cx).plugged_device(udid).map(Chosen::from) {
            let (udid, name) = (plugged.udid.clone(), plugged.name.clone());
            let pane = opened(caller, plugged, cx)?;
            return Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| until_ready(udid, name, pane, started, cx).await)));
        }
    }
    let set = devices::set_dir().ok_or_else(|| ControlError::internal("Koyo has no folder to keep its devices in."))?;
    let room = Room {
        starting: devices::devices(cx).read(cx).starting(cx),
        at_once: devices::at_once(cx),
    };
    let (last, asked) = (last_device(caller, cx), super::simctl_asked(cx));
    let chosen = cx.background_executor().spawn(async move {
        asked()?;
        choose(&set, &input, last.as_deref(), &room)
    });
    let caller = caller.clone();
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let simulator = chosen.await?;
        let pane = cx.update(|cx| opened(&caller, Chosen::from(&simulator), cx))?;
        until_ready(simulator.udid, simulator.name, pane, started, cx).await
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
