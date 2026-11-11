//! The device tools (`docs/devices.md` 9): each finds its device ([`which`]), asks its host
//! through the device's link, and answers what the host says in app control's words. An act
//! is refused while the person uses the device in its tab.

mod acts;
mod apps;
mod lend;
mod looks;
mod opening;

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{App, AsyncApp, Entity, Global};
use koyo_app_control::ops::shared::PaneId;
use koyo_app_control::{ControlError, ErrorCode};
use koyo_device_wire::{Answer, Ask, Refusal, RefusalCode};
use serde_json::Value;

pub(crate) use acts::{button, key, pinch, rotate, scroll, swipe, tap, type_text};
pub(crate) use apps::{apps, install, launch, open_url, terminate};
pub(crate) use looks::{list, screenshot, snapshot, wait};
pub(crate) use opening::{close, open};

use super::{Exec, main_shell};
use crate::browser::control::{Mode, Refused};
use crate::control::Caller;
use crate::devices::{self, Link};

/// How long each ask may take before the call fails `timeout`, each inside the client's 100 s.
const SCREENSHOT: Duration = Duration::from_secs(15);
const SNAPSHOT: Duration = Duration::from_secs(20);
const ACT: Duration = Duration::from_secs(30);
const APPS: Duration = Duration::from_secs(30);
/// A launch or a link: `simctl`'s (28 s at most), then until the launched app is in front (10 s)
/// and what it opened has drawn (10 s).
const OPENING: Duration = Duration::from_secs(55);
const INSTALL: Duration = Duration::from_secs(90);
/// A wait's ask has this beyond the wait itself.
const WAIT_SLACK: Duration = Duration::from_secs(5);

/// The device each caller acted on last, by its id.
#[derive(Default)]
struct LastDevices(HashMap<String, String>);

impl Global for LastDevices {}

/// The device a tool works on.
pub(super) struct Target {
    pub link: Entity<Link>,
    pub udid: String,
    pub name: String,
}

/// Every device tab in the window, as its pane and its device's id, and the device of the tab
/// shown.
fn device_tabs(cx: &App) -> (Vec<(u64, String)>, Option<String>) {
    let Some((_, shell)) = main_shell(cx) else {
        return (Vec::new(), None);
    };
    let workspace = &shell.read(cx).workspace;
    let udid = |pane: &crate::model::Pane| Some(pane.device_view()?.read(cx).chosen()?.udid.clone());
    let mut tabs = Vec::new();
    for project in &workspace.projects {
        for unit in &project.branches {
            for tab in &unit.tabs {
                tabs.extend(tab.panes().into_iter().filter_map(|pane| Some((pane.id, udid(pane)?))));
            }
        }
    }
    let shown = workspace.tab_shown().and_then(|tab| tab.panes().into_iter().find_map(udid));
    (tabs, shown)
}

/// The pane of the tab that shows the device `udid`, if one does.
pub(super) fn pane_of(udid: &str, cx: &App) -> Option<u64> {
    panes_of(udid, cx).into_iter().next()
}

/// The panes of every tab that shows the device `udid`.
pub(super) fn panes_of(udid: &str, cx: &App) -> Vec<u64> {
    (device_tabs(cx).0.into_iter())
        .filter_map(|(pane, shown)| (shown == udid).then_some(pane))
        .collect()
}

/// The device `device` or `pane` names; with neither, the one `caller` acted on last, else the
/// shown tab's, else the only one running. A real device not lent to the caller is asked for
/// ([`lend::usable`]).
pub(super) fn which(caller: &Caller, device: Option<&str>, pane: Option<PaneId>, cx: &mut App) -> Result<Target, ControlError> {
    which_for(caller, (device, pane), None, cx)
}

/// [`which`], for `ask`: what a real device refuses anyway is refused before the person is
/// asked to lend it.
fn which_for(caller: &Caller, (device, pane): Named<'_>, ask: Option<&Ask>, cx: &mut App) -> Result<Target, ControlError> {
    let udid = named(caller, device, pane, cx)?;
    let refusal = ask.and_then(koyo_device_wire::real::refuses).filter(|_| lend::real(&udid, cx).is_some());
    if let Some(refusal) = refusal {
        return Err(refused(refusal));
    }
    lend::usable(caller, &udid, cx)?;
    let registry = devices::devices(cx).read(cx);
    match registry.existing(&udid) {
        Some(link) => {
            let name = link.read(cx).name.clone();
            Ok(Target { link, udid, name })
        }
        None if registry.simulator(&udid).is_some() || registry.plugged_device(&udid).is_some() => Err(
            ControlError::device_unavailable(format!("{udid} isn't open: device_open opens it.")).retryable(false),
        ),
        None => Err(ControlError::not_found(format!(
            "No device {udid} among Koyo's: device_list lists them."
        ))),
    }
}

/// The id of the device `device` or `pane` names; with neither, of the one `caller` acted on
/// last while a tab still shows it, else the shown tab's, else the only one running: of those,
/// only what the caller may use unasked.
fn named(caller: &Caller, device: Option<&str>, pane: Option<PaneId>, cx: &App) -> Result<String, ControlError> {
    match (device, pane) {
        (Some(_), Some(_)) => Err(ControlError::invalid_input("Give `device` or `pane`, not both.")),
        (Some(udid), None) => Ok(udid.to_owned()),
        (None, Some(pane)) => (device_tabs(cx).0.into_iter())
            .find_map(|(id, udid)| (id == pane).then_some(udid))
            .ok_or_else(|| ControlError::not_found(format!("Pane {pane} shows no device; device_list lists them."))),
        (None, None) => {
            let (tabs, shown) = device_tabs(cx);
            let agent = caller.actor();
            let allowed = |udid: &String| lend::usable_unasked(&agent, udid, cx);
            let last = last_device(caller, cx).filter(|udid| tabs.iter().any(|(_, shown)| shown == udid) && allowed(udid));
            let registry = devices::devices(cx).read(cx);
            let running: Vec<&Entity<Link>> = (registry.links())
                .filter(|link| link.read(cx).running_or_starting() && allowed(&link.read(cx).udid))
                .collect();
            match (last.or(shown.filter(allowed)), running.as_slice()) {
                (Some(udid), _) => Ok(udid),
                (None, [only]) => Ok(only.read(cx).udid.clone()),
                (None, []) => Err(ControlError::not_found(
                    "No device is open: device_open opens one, device_list lists them.",
                )),
                (None, many) => {
                    let named: Vec<String> = (many.iter())
                        .map(|link| format!("{} ({})", link.read(cx).name, link.read(cx).udid))
                        .collect();
                    Err(ControlError::ambiguous("Several devices are open; name one with `device`.", &named))
                }
            }
        }
    }
}

/// Notes the device `udid` as the one `caller` acted on last.
fn remember(caller: &Caller, udid: &str, cx: &mut App) {
    (cx.default_global::<LastDevices>().0).insert(caller.key(), udid.to_owned());
}

/// The device `caller` acted on last.
fn last_device(caller: &Caller, cx: &App) -> Option<String> {
    cx.try_global::<LastDevices>()?.0.get(&caller.key()).cloned()
}

/// Why `link` can't be asked anything now, in app control's words: the person's to put right
/// when what it needs is missing; else not to be retried, but opened again.
pub(super) fn not_running(link: &Link) -> Option<ControlError> {
    let (why, person) = link.not_running()?;
    Some(if person {
        needs_person(&why)
    } else {
        ControlError::device_unavailable(format!("{why} device_open starts it again.")).retryable(false)
    })
}

/// Whether `simctl` may be asked, to be called off the main thread: at once when Koyo knows
/// Xcode and its components are there, else after looking. An agent's tool is never the first
/// `simctl` call, which would install Xcode's components and may ask the person's password.
fn simctl_asked(cx: &App) -> impl FnOnce() -> Result<(), ControlError> + Send + 'static {
    use devices::Need;
    let known = (devices::devices(cx).read(cx).found.as_ref())
        .is_some_and(|found| !matches!(found.need(), Some(Need::Xcode | Need::Components)));
    move || {
        if known {
            return Ok(());
        }
        devices::needs::before_simctl().map_err(|why| needs_person(&why))
    }
}

/// What only the person can put right, as the message tells the agent.
pub(super) fn needs_person(why: &str) -> ControlError {
    ControlError::device_needs_person(format!("{why} Tell the person: Settings → Devices walks them through it."))
}

/// Asks `target`'s host, and answers what `answered` makes of its answer.
fn asking(
    target: &Target,
    ask: Ask,
    limit: Duration,
    cx: &mut App,
    answered: impl FnOnce(Answer) -> Result<Value, ControlError> + 'static,
) -> Exec {
    if let Some(error) = not_running(target.link.read(cx)) {
        return Exec::Ready(Err(error));
    }
    let asked = target.link.update(cx, |link, cx| link.ask(ask, limit, cx));
    Exec::Later(cx.spawn(async move |_: &mut AsyncApp| asked.await.map_err(refused).and_then(answered)))
}

/// `target` is `caller`'s from now on, unless the person holds it.
fn claim(caller: &Caller, target: &Target, cx: &mut App) -> Result<(), ControlError> {
    match target.link.update(cx, |link, cx| link.agent_acts(caller.actor(), cx)) {
        Ok(()) => {
            remember(caller, &target.udid, cx);
            Ok(())
        }
        Err(Refused::PersonHasControl {
            mode: Mode::Implicit, ..
        }) => Err(person_using(&target.name)),
        Err(_) => Err(ControlError::new(
            ErrorCode::PersonHasControl,
            format!(
                "The person has taken control of {}. Don't retry or wait for it; do something else until they give it back.",
                target.name
            ),
        )),
    }
}

/// What a tool names its device by: `device`, or `pane`.
type Named<'a> = (Option<&'a str>, Option<PaneId>);

/// Asks the host of the device `named` names to act for `caller`, whose device it is from then
/// on unless the person holds it; `answered`, made for the device, reads the answer.
fn act_on<F>(
    caller: &Caller,
    (device, pane): Named<'_>,
    ask: Result<Ask, ControlError>,
    limit: Duration,
    cx: &mut App,
    answered: impl FnOnce(&Target) -> F,
) -> Result<Exec, ControlError>
where
    F: FnOnce(Answer) -> Result<Value, ControlError> + 'static,
{
    let ask = ask?;
    let target = which_for(caller, (device, pane), Some(&ask), cx)?;
    claim(caller, &target, cx)?;
    let answered = answered(&target);
    Ok(asking(&target, ask, limit, cx, answered))
}

/// The person acted in the device's tab lately.
fn person_using(name: &str) -> ControlError {
    ControlError::new(
        ErrorCode::PersonHasControl,
        format!("The person is using {name}; it comes back on its own a few seconds after they stop. Wait, then look again before acting."),
    )
    .retryable(true)
}

/// A refusal of the host's, in app control's words; its message as it is.
pub(super) fn refused(refusal: Refusal) -> ControlError {
    let Refusal { code, message } = refusal;
    match code {
        RefusalCode::NotReady | RefusalCode::Gone => ControlError::device_unavailable(message),
        RefusalCode::Locked => ControlError::device_needs_person(message),
        RefusalCode::NotFound => ControlError::not_found(message),
        RefusalCode::Invalid => ControlError::invalid_input(message),
        RefusalCode::Unsupported => ControlError::new(ErrorCode::Unsupported, message),
        RefusalCode::TimedOut => ControlError::timeout(message),
        // Cut short by the person: theirs for now, as when they act before it starts.
        RefusalCode::PersonActing => ControlError::new(ErrorCode::PersonHasControl, message).retryable(true),
        RefusalCode::Failed => ControlError::internal(message),
    }
}

/// An answer that isn't the one asked for.
fn unexpected(answer: &Answer) -> ControlError {
    let what = match answer {
        Answer::Done => "done",
        Answer::Acted { .. } => "an act",
        Answer::Screenshot { .. } => "a screenshot",
        Answer::Snapshot { .. } => "a snapshot",
        Answer::Waited { .. } => "a wait",
        Answer::Apps { .. } => "its apps",
    };
    ControlError::internal(format!("The device answered with {what}, which wasn't asked for."))
}

#[cfg(test)]
mod tests;
