//! Looking at devices: the list, a screenshot, a snapshot and a wait. None takes the device
//! from anyone.

use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::device::{
    DeviceInfo, DeviceListOutput, DeviceScreen, DeviceScreenshotInput, DeviceScreenshotOutput,
    DeviceSnapshotInput, DeviceSnapshotOutput, DeviceWaitInput, DeviceWaitOutput, SnapshotSource,
};
use koyo_device_wire::{Answer, Ask, Orientation, Screen, Source, Step};

use super::{SCREENSHOT, SNAPSHOT, WAIT_SLACK, asking, pane_of, unexpected, which};
use crate::browser::control::Holder;
use crate::control::Caller;
use crate::control::exec::{Exec, output};
use crate::devices::connected::Connected;
use crate::devices::{self, Link, Standing};

/// How a picture faces, as the tools name it.
pub(super) fn facing(orientation: Orientation) -> &'static str {
    match orientation {
        Orientation::Portrait => "portrait",
        Orientation::PortraitUpsideDown => "portraitUpsideDown",
        Orientation::LandscapeLeft => "landscapeLeft",
        Orientation::LandscapeRight => "landscapeRight",
    }
}

pub(super) fn screen(screen: Screen) -> DeviceScreen {
    DeviceScreen {
        width: screen.size.width,
        height: screen.size.height,
        scale: screen.scale,
        orientation: facing(screen.orientation).into(),
    }
}

/// Where a start is, as the tools name it.
pub(super) fn step(step: Step) -> &'static str {
    match step {
        Step::Booting { first: true } => "firstBoot",
        Step::Booting { first: false } => "booting",
        Step::Picture => "picture",
        Step::Input => "input",
    }
}

/// How a device stands, as `device_list` says it, and what the person must put right.
pub(super) fn standing(link: &Link) -> (&'static str, Option<String>) {
    match link.standing() {
        Standing::Ended { missing: None, why } => ("gone", Some(why.to_owned())),
        Standing::Ended {
            missing: Some(missing),
            why,
        } => (if missing.needs_person() { "needsPerson" } else { "failed" }, Some(why.to_owned())),
        Standing::Off => ("off", None),
        Standing::Failed(why) => ("failed", Some(why.to_owned())),
        Standing::Ready(_) => ("ready", None),
        Standing::Locked(_) => ("locked", None),
        Standing::Starting(_) => ("starting", None),
    }
}

/// Who drives a device now: an agent's name, or `person`.
fn controller(link: &Link) -> Option<String> {
    match &link.control.holder {
        Some(Holder::Human { .. }) => Some("person".into()),
        Some(Holder::Agent { actor, .. }) => Some(actor.label.clone()),
        None => None,
    }
}

/// A device as it's listed, as it stands: a simulator, unless the caller says otherwise.
fn info(udid: &str, name: &str, system: &str, cx: &App) -> DeviceInfo {
    let link = devices::devices(cx).read(cx).existing(udid);
    let link = link.as_ref().map(|link| link.read(cx));
    let (state, missing) = link.map_or(("off", None), standing);
    DeviceInfo {
        device: udid.to_owned(),
        name: name.to_owned(),
        kind: "simulator".into(),
        system: system.to_owned(),
        state: state.into(),
        missing,
        pane: pane_of(udid, cx),
        controller: link.and_then(controller),
        screen: link.and_then(Link::screen).map(screen),
        set: None,
    }
}

/// A real device plugged in: not started, it says the step it misses.
fn plugged_info(device: &Connected, cx: &App) -> DeviceInfo {
    let mut info = DeviceInfo {
        kind: "device".into(),
        ..info(&device.udid, &device.name, &device.system, cx)
    };
    if let (true, Some((_, said))) = (info.state == "off", &device.missing) {
        (info.state, info.missing) = ("needsPerson".into(), Some(said.clone()));
    }
    info
}

/// `device_list`: Koyo's simulators, read from its set afresh, and the iPhones and iPads plugged
/// in (as the watcher keeps them, else listed now), each as it stands.
pub(crate) fn list(cx: &mut App) -> Result<Exec, ControlError> {
    let set = devices::set_dir().ok_or_else(|| ControlError::internal("Koyo has no folder to keep its devices in."))?;
    let (path, asked) = (set.clone(), super::simctl_asked(cx));
    let listed = cx.background_executor().spawn(async move {
        asked()?;
        devices::simulators::list(&path).map_err(ControlError::internal)
    });
    let registry = devices::devices(cx);
    let kept = registry.read(cx).watching() && matches!(registry.read(cx).connected, Some(Ok(_)));
    let since = Instant::now();
    let plugged = (!kept).then(|| cx.background_executor().spawn(async move { devices::connected::list() }));
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let (simulators, plugged) = (listed.await?, match plugged {
            Some(listing) => Some(listing.await),
            None => None,
        });
        cx.update(|cx| {
            if let Some(plugged) = plugged {
                registry.update(cx, |registry, cx| registry.connected_afresh(plugged, since, cx));
            }
            let set = set.display().to_string();
            let mut devices: Vec<DeviceInfo> = (simulators.iter())
                .map(|simulator| DeviceInfo {
                    set: Some(set.clone()),
                    ..info(&simulator.udid, &simulator.name, &simulator.system, cx)
                })
                .collect();
            devices.extend(registry.read(cx).plugged().iter().map(|device| plugged_info(device, cx)));
            // The launcher and the picker read the same list.
            registry.update(cx, |registry, cx| registry.listed_afresh(simulators, cx));
            output(&DeviceListOutput { devices })
        })
    })))
}

pub(crate) fn screenshot(caller: &Caller, input: &DeviceScreenshotInput, cx: &mut App) -> Result<Exec, ControlError> {
    let target = which(caller, input.device.as_deref(), input.pane, cx)?;
    let scale = input.scale()?;
    let name = target.name.clone();
    Ok(asking(&target, Ask::Screenshot { scale }, SCREENSHOT, cx, move |answer| match answer {
        Answer::Screenshot { png, size, scale } => output(&DeviceScreenshotOutput {
            mime_type: "image/png".into(),
            data: STANDARD.encode(png.as_ref()),
            name,
            width: size.width,
            height: size.height,
            scale,
        }),
        other => Err(unexpected(&other)),
    }))
}

pub(crate) fn snapshot(caller: &Caller, input: &DeviceSnapshotInput, cx: &mut App) -> Result<Exec, ControlError> {
    let target = which(caller, input.device.as_deref(), input.pane, cx)?;
    let text = input.source == Some(SnapshotSource::Text);
    Ok(asking(&target, Ask::Snapshot { text }, SNAPSHOT, cx, |answer| match answer {
        Answer::Snapshot {
            tree,
            source,
            elements,
            truncated,
        } => output(&DeviceSnapshotOutput {
            tree,
            source: match source {
                Source::Accessibility => "accessibility",
                Source::Text => "text",
            }
            .into(),
            elements,
            truncated,
        }),
        other => Err(unexpected(&other)),
    }))
}

pub(crate) fn wait(caller: &Caller, input: &DeviceWaitInput, cx: &mut App) -> Result<Exec, ControlError> {
    let target = which(caller, input.device.as_deref(), input.pane, cx)?;
    let (quiet_ms, timeout_ms) = (input.quiet_ms(), input.timeout_ms());
    let ask = Ask::Wait {
        quiet_ms: u32::try_from(quiet_ms).unwrap_or(u32::MAX),
        timeout_ms: u32::try_from(timeout_ms).unwrap_or(u32::MAX),
    };
    let limit = std::time::Duration::from_millis(timeout_ms) + WAIT_SLACK;
    Ok(asking(&target, ask, limit, cx, |answer| match answer {
        Answer::Waited { still, millis } => output(&DeviceWaitOutput {
            still,
            millis: u64::from(millis),
        }),
        other => Err(unexpected(&other)),
    }))
}
