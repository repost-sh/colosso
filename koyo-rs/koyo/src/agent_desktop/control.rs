//! Who drives the agent desktop (`docs/agent-desktop.md` 9): its lease ([`Leased`]), a
//! device's, for the one desktop that agents share. The bar at the tab's bottom says who has it,
//! the same bar a page has.

use gpui_kit::{AnyElement, ClickEvent, Context, ElementId, Window};

use super::Desk;
use super::view::DeskView;
use crate::browser::control::{Actor, Lease, Leased, Mode};
use crate::held::{Held, OnClick};

impl Leased for Desk {
    fn lease(&self) -> &Lease {
        &self.lease
    }

    fn lease_mut(&mut self) -> &mut Lease {
        &mut self.lease
    }

    fn named(&self) -> &'static str {
        "the agent desktop"
    }
}

impl Desk {
    /// Whether the person holds the desktop: an agent's act is refused then.
    pub fn person_holds(&self) -> bool {
        self.lease.control.person_holds()
    }
}

impl DeskView {
    /// The bar that says who has the desktop while it's contested: an agent using it, with
    /// the person's way to take it; or the person having taken it from an agent, with the
    /// way to give it back.
    pub(super) fn control_bar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (agent, displaced, using) = {
            let control = &self.desk.read(cx).lease.control;
            let label = |actor: &Actor| actor.label.clone();
            (
                control.agent().map(label),
                control.displaced.as_ref().map(label),
                control.person_using(),
            )
        };
        let (text, id, label, on_click): (_, _, _, OnClick) = if let Some(agent) = agent {
            let take = cx.listener(|this, _: &ClickEvent, _, cx| {
                (this.desk).update(cx, |desk, cx| desk.person_acts(Mode::Explicit, cx));
            });
            let text = format!("{agent} is using the agent desktop");
            (text, "desktop-take-control", "Take control", Box::new(take))
        } else {
            let displaced = displaced?;
            let give_back = cx.listener(|this, _: &ClickEvent, _, cx| {
                this.desk.update(cx, Desk::give_back);
            });
            let text = if using {
                format!("{displaced} waits while you use the agent desktop")
            } else {
                format!("You took over the agent desktop from {displaced}")
            };
            (text, "desktop-give-back", "Give back", Box::new(give_back))
        };
        let held = Held {
            text,
            id,
            label,
            on_click,
        };
        let glass = ElementId::Name("agent-desktop-control-glass".into());
        Some(crate::held::bar(glass, held, None, window, cx))
    }
}
