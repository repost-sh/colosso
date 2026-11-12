//! The person's own iPhone or iPad lent to agents (`docs/devices.md` 9): by their click on
//! Koyo's sheet when an agent first asks (`control/exec/device/lend.rs`), or ahead of that from
//! the device's tab. Kept in the app by device and agent, so a host that restarts keeps it; it
//! ends at Take It Back, when the device's tab closes, when it's unplugged and when Koyo quits.
//! The person's Don't Allow holds ten minutes for that agent and device.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{AnyElement, ClickEvent, Context, IntoElement as _, ParentElement as _, Styled as _, div};

use super::Devices;
use super::view::DeviceView;
use crate::browser::control::Actor;
use crate::modal::{Button as Push, Tone};

/// To whom a device is lent.
#[derive(Default)]
struct Lent {
    /// To every agent: lent ahead from its tab.
    everyone: bool,
    agents: Vec<Actor>,
}

/// What's lent, refused and being asked, by device. An agent is its caller's key and the name
/// it goes by together: another program in the same terminal isn't the one the person allowed.
#[derive(Default)]
pub struct Lends {
    lent: HashMap<String, Lent>,
    declined: HashMap<(String, String, String), Instant>,
    /// The agent each device's sheet is up for: one at a time.
    asking: HashMap<String, Actor>,
}

/// The key a refusal is kept by.
fn refusal(udid: &str, agent: &Actor) -> (String, String, String) {
    (udid.to_owned(), agent.id.clone(), agent.label.clone())
}

impl Lends {
    /// Whether `udid` is lent to `agent`.
    pub fn lent_to(&self, udid: &str, agent: &Actor) -> bool {
        self.lent.get(udid).is_some_and(|lent| lent.everyone || lent.agents.contains(agent))
    }

    /// Whether `udid` is lent to anyone.
    pub fn is_lent(&self, udid: &str) -> bool {
        self.lent.contains_key(udid)
    }

    /// Whom `udid` is lent to, in words: `Claude`, `Claude and Codex`, `agents`.
    pub fn to_whom(&self, udid: &str) -> Option<String> {
        let lent = self.lent.get(udid)?;
        let names: Vec<&str> = lent.agents.iter().map(|agent| agent.label.as_str()).collect();
        Some(match names.split_last() {
            Some((last, rest)) if !lent.everyone => match rest {
                [] => (*last).to_owned(),
                _ => format!("{} and {last}", rest.join(", ")),
            },
            _ => "agents".into(),
        })
    }

    /// Lends `udid` to `agent`, or with `None` to every agent.
    pub fn lend(&mut self, udid: &str, agent: Option<Actor>) {
        let lent = self.lent.entry(udid.to_owned()).or_default();
        match agent {
            Some(agent) if !lent.agents.contains(&agent) => lent.agents.push(agent),
            Some(_) => {}
            None => lent.everyone = true,
        }
    }

    /// Ends every lend of `udid`; whether there was one.
    fn end(&mut self, udid: &str) -> bool {
        self.lent.remove(udid).is_some()
    }

    /// Whether the person said no to `agent` having `udid` within `memory` of `now`.
    pub fn declined(&self, udid: &str, agent: &Actor, now: Instant, memory: Duration) -> bool {
        (self.declined.get(&refusal(udid, agent))).is_some_and(|at| now.saturating_duration_since(*at) < memory)
    }

    pub fn decline(&mut self, udid: &str, agent: &Actor, now: Instant) {
        self.declined.insert(refusal(udid, agent), now);
    }

    /// The agent `udid`'s sheet is up for.
    pub fn asking(&self, udid: &str) -> Option<&Actor> {
        self.asking.get(udid)
    }

    /// A sheet is up for `udid` and `agent`.
    pub fn ask(&mut self, udid: &str, agent: Actor) {
        self.asking.insert(udid.to_owned(), agent);
    }

    /// The sheet for `udid` was answered, or went.
    pub fn asked(&mut self, udid: &str) {
        self.asking.remove(udid);
    }
}

impl Devices {
    /// The lend of `udid` ends with it (stopped, or unplugged): its host's stop let the agent go.
    pub(super) fn lend_ends(&mut self, udid: &str, cx: &mut Context<Self>) {
        if self.lends.end(udid) {
            cx.notify();
        }
    }

    /// The person takes `udid` back: no agent may use it, and what they had under way is called
    /// off. An agent's next call asks the person again (QR9).
    pub fn take_back(&mut self, udid: &str, cx: &mut Context<Self>) {
        if !self.lends.end(udid) {
            return;
        }
        if let Some(link) = self.existing(udid) {
            link.update(cx, |link, cx| link.revoke("The person took their device back.", cx));
        }
        cx.notify();
    }
}

impl DeviceView {
    /// A real device lent: whose it is and to whom, with the person's way to take it back, as the
    /// agent desktop says an app of theirs is lent: the same notice over the top of the pane.
    pub(super) fn lend_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let chosen = self.device.as_ref()?;
        let whom = super::devices(cx).read(cx).lends.to_whom(&chosen.udid)?;
        let udid = chosen.udid.clone();
        let back = move |_: &ClickEvent, _: &mut gpui_kit::Window, cx: &mut gpui_kit::App| {
            super::devices(cx).update(cx, |devices, cx| devices.take_back(&udid, cx));
        };
        let said = format!("{} is yours, lent to {whom} until you take it back.", chosen.name);
        let push = Push::new("device-take-back", "Take It Back", Tone::Primary).on_click(back);
        let notice = crate::held::notice(said, push, crate::theme::palette(cx));
        Some(div().absolute().top_2().left_0().w_full().flex().justify_center().child(notice).into_any_element())
    }

    /// Lends the device shown to every agent, ahead of being asked.
    pub(super) fn lend_ahead(&mut self, cx: &mut Context<Self>) {
        let Some(udid) = self.device.as_ref().filter(|device| device.phone).map(|device| device.udid.clone()) else {
            return;
        };
        super::devices(cx).update(cx, |devices, cx| {
            devices.lends.lend(&udid, None);
            cx.notify();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, label: &str) -> Actor {
        Actor {
            id: id.into(),
            label: label.into(),
        }
    }

    #[test]
    fn a_lend_is_by_device_and_agent_and_says_to_whom() {
        let mut lends = Lends::default();
        let claude = agent("chat:1", "Claude");
        lends.lend("PAD", Some(claude.clone()));
        lends.lend("PAD", Some(claude.clone()));
        assert!(lends.lent_to("PAD", &claude));
        assert!(!lends.lent_to("PAD", &agent("chat:2", "Claude")), "another agent");
        assert!(!lends.lent_to("PAD", &agent("chat:1", "Codex")), "another program where Claude was");
        assert!(!lends.lent_to("PHONE", &claude), "another device");
        assert_eq!(lends.to_whom("PAD").as_deref(), Some("Claude"));
        lends.lend("PAD", Some(agent("t1", "Codex")));
        assert_eq!(lends.to_whom("PAD").as_deref(), Some("Claude and Codex"));
        lends.lend("PAD", None);
        assert!(lends.lent_to("PAD", &agent("chat:9", "Gemini")));
        assert_eq!(lends.to_whom("PAD").as_deref(), Some("agents"));
        assert!(lends.end("PAD"));
        assert!(!lends.is_lent("PAD"));
        let now = Instant::now();
        lends.decline("PAD", &claude, now);
        let memory = Duration::from_secs(600);
        assert!(lends.declined("PAD", &claude, now, memory) && !lends.declined("PAD", &agent("chat:2", "Claude"), now, memory));
        assert!(!lends.declined("PAD", &claude, now + memory, memory), "not for good");
    }
}
