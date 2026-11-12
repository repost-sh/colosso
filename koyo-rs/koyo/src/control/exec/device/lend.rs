//! A real device lent by the person's own click (`docs/devices.md` 9): an agent's first call on
//! one that isn't lent to it, reads included, puts a sheet in Koyo's window and returns at once,
//! as a person may take minutes and a call has a hundred seconds. Allow lends it to that agent;
//! Don't Allow keeps it from asking again for ten minutes. Koyo can't tell an agent's words from
//! the person's, so only their click lends it; and a read raises the sheet too, so a picture of
//! their phone is never taken unasked.

use gpui_kit::{App, AsyncApp, PromptLevel};
use koyo_app_control::ControlError;

use crate::browser::control::Actor;
use crate::control::activity::ActivityKind;
use crate::control::exec::{main_shell, window_closed};
use crate::control::{Caller, Control, confirm};
use crate::devices::{self, Chosen};

/// A real device's name and kind, when `udid` is one: plugged in, or open in a tab.
pub(super) fn real(udid: &str, cx: &App) -> Option<(String, &'static str)> {
    let registry = devices::devices(cx).read(cx);
    if let Some(device) = registry.plugged_device(udid) {
        return Some((device.name.clone(), Chosen::from(device).kind()));
    }
    // Open in a tab but not plugged in: which kind isn't listed.
    let link = registry.existing(udid).filter(|link| link.read(cx).phone)?;
    Some((link.read(cx).name.clone(), "device"))
}

/// Whether `caller` may use `udid` without asking: a simulator of Koyo's, or a real device lent
/// to it. A call that names no device finds only these.
pub(super) fn usable_unasked(caller: &Actor, udid: &str, cx: &App) -> bool {
    let registry = devices::devices(cx).read(cx);
    let real = registry.plugged_device(udid).is_some() || registry.existing(udid).is_some_and(|link| link.read(cx).phone);
    !real || registry.lends.lent_to(udid, caller)
}

/// Whether `caller` may use `udid`: a simulator of Koyo's, or a real device lent to it. One not
/// lent puts the sheet up, unless the person said no lately or a sheet is up for it already,
/// and the call answers at once with what the agent should do.
pub(super) fn usable(caller: &Caller, udid: &str, cx: &mut App) -> Result<(), ControlError> {
    let Some((name, kind)) = real(udid, cx) else {
        return Ok(());
    };
    // Listed with a step missing: said at once, with no question put to the person for a device
    // that can't be used, and nothing sent to it.
    let registry = devices::devices(cx);
    if let Some((_, why)) = registry.read(cx).misses(udid) {
        return Err(super::needs_person(why));
    }
    let agent = caller.actor();
    let (now, memory) = (cx.background_executor().now(), Control::get(cx).timeouts.denial_memory);
    let lends = &registry.read(cx).lends;
    if lends.lent_to(udid, &agent) {
        return Ok(());
    }
    if lends.declined(udid, &agent, now, memory) {
        return Err(ControlError::user_declined(format!(
            "The person didn't let you use their {kind} “{name}”. Don't ask again for a while: tell them what you need it \
             for; they can also lend it from its tab."
        )));
    }
    match lends.asking(udid) {
        None => ask(&agent, udid, &name, kind, cx)?,
        Some(asked) if *asked != agent => {
            return Err(ControlError::device_needs_person(format!(
                "Koyo is asking the person, in its window, whether another agent may use their {kind} “{name}”. \
                 Call again once they've answered."
            )));
        }
        Some(_) => {}
    }
    Err(ControlError::device_needs_person(format!(
        "Koyo is asking the person, in its window, whether you may use their {kind} “{name}”. Tell them why you need \
         it and that the question is waiting there. Call again when they have answered."
    )))
}

/// Puts the sheet up for `agent` and `udid`, and lends or refuses by the person's click. It
/// stays until answered: there's no taking one down.
fn ask(agent: &Actor, udid: &str, name: &str, kind: &str, cx: &mut App) -> Result<(), ControlError> {
    let (window, _) = main_shell(cx).ok_or_else(window_closed)?;
    let question = format!("Let “{}” use your {kind} “{name}”?", agent.label);
    let detail = format!(
        "It will see the {kind}'s screen, and tap, type and open apps on it as you would, with everything on it \
         that you're signed in to. You can watch in its tab. Take it back there, or pull the cable, to end \
         it."
    );
    let answer = window
        .update(cx, |_, window, cx| window.prompt(PromptLevel::Warning, &question, Some(&detail), &["Allow", "Don't Allow"], cx))
        .map_err(|error| ControlError::internal(format!("Koyo couldn't ask the person: {error}")))?;
    let registry = devices::devices(cx);
    registry.update(cx, |registry, _| registry.lends.ask(udid, agent.clone()));
    let (udid, agent) = (udid.to_owned(), agent.clone());
    cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = confirm::decision(answer.await);
        cx.update(|cx| {
            let kind = ActivityKind::Confirmation {
                operation: "device_lend".into(),
                question,
                decision,
            };
            Control::get_mut(cx).activity.record(agent.id.clone(), kind);
            let now = cx.background_executor().now();
            registry.update(cx, |registry, cx| {
                registry.lends.asked(&udid);
                match decision {
                    // Unplugged while the sheet was up, it isn't lent: it comes back unlent.
                    crate::control::activity::Decision::Allowed if registry.plugged_device(&udid).is_none() && registry.existing(&udid).is_none() => {}
                    crate::control::activity::Decision::Allowed => registry.lends.lend(&udid, Some(agent)),
                    crate::control::activity::Decision::Denied => registry.lends.decline(&udid, &agent, now),
                    crate::control::activity::Decision::TimedOut => {}
                }
                cx.notify();
            });
        });
    })
    .detach();
    Ok(())
}
