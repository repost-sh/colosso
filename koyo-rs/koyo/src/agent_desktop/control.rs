//! Who drives the agent desktop (`docs/agent-desktop.md` 9): the lease a browser page has
//! (`browser::control`), for the one desktop. The person pre-empts any agent: by acting in
//! the tab, which holds the desktop while they're at it and [`IMPLICIT_HOLD`] after; or with
//! Take control, which holds it until they give it back. An agent's act takes the desktop
//! for that agent, from another agent too (there's one desktop, and agents share it), and
//! an agent that rests a while has let it go. The bar at the tab's bottom says who has it,
//! the same bar a page has.

use std::time::{Duration, Instant};

use gpui_kit::{AnyElement, ClickEvent, Context, ElementId, Window};

use super::Desk;
use super::view::DeskView;
use crate::browser::control::{Actor, Control, IMPLICIT_HOLD, Mode, Refused};
use crate::held::{Held, OnClick};

/// How long after its last act an agent still has the desktop.
pub const RESTS_AFTER: Duration = Duration::from_secs(10);

/// What comes to its end by itself: the person's hold by acting, an agent's by resting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Hold,
    Rest,
}

impl Desk {
    /// The person acts in the tab, or asks for the desktop: theirs from now on, whoever
    /// drove it, and for [`IMPLICIT_HOLD`] past this act unless they asked for it.
    pub fn person_acts(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let held = self.control.holder.clone();
        if let Some(displaced) = self.control.person_takes(mode) {
            log::info!("the person took the agent desktop from {}", displaced.label);
        }
        if self.control.person_using() {
            self.ends(Ends::Hold, IMPLICIT_HOLD, cx);
        } else {
            self.lapse = None;
        }
        if self.control.holder != held {
            cx.notify();
        }
    }

    /// The person gives the desktop back: nobody holds it, and an agent may act again.
    pub fn give_back(&mut self, cx: &mut Context<Self>) {
        self.lapse = None;
        self.control.person_gives_back();
        cx.notify();
    }

    /// An agent is about to act: refused while the person holds the desktop, and its own
    /// otherwise, until it has rested [`RESTS_AFTER`].
    pub fn agent_acts(&mut self, actor: Actor, cx: &mut Context<Self>) -> Result<(), Refused> {
        let held = self.control.holder.clone();
        self.control.agent_claims(actor, true, None)?;
        self.ends(Ends::Rest, RESTS_AFTER, cx);
        if self.control.holder != held {
            cx.notify();
        }
        Ok(())
    }

    /// Whether the person holds the desktop: an agent's act is refused then.
    pub fn person_holds(&self) -> bool {
        self.control.person_holds()
    }

    /// Nobody holds a desktop that's gone.
    pub(super) fn released(&mut self) {
        self.control = Control::default();
        (self.lapse, self.rest) = (None, None);
    }

    fn end(&mut self, which: Ends) -> &mut Option<(Instant, gpui_kit::Task<()>)> {
        match which {
            Ends::Hold => &mut self.lapse,
            Ends::Rest => &mut self.rest,
        }
    }

    /// Has `which` end `after` from now. One task waits on each however often it's asked:
    /// each asking only moves its deadline on.
    fn ends(&mut self, which: Ends, after: Duration, cx: &mut Context<Self>) {
        let until = cx.background_executor().now() + after;
        if let Some((deadline, _)) = self.end(which) {
            *deadline = until;
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            // Sleeps to the deadline, and on to it again each time it was moved.
            loop {
                let left = this.update(cx, |this, cx| {
                    let (until, _) = this.end(which).as_ref()?;
                    Some(until.saturating_duration_since(cx.background_executor().now()))
                });
                match left {
                    Ok(Some(left)) if !left.is_zero() => {
                        cx.background_executor().timer(left).await;
                    }
                    Ok(Some(_)) => break,
                    Ok(None) | Err(_) => return,
                }
            }
            let _ = this.update(cx, |this, cx| {
                *this.end(which) = None;
                match which {
                    Ends::Hold => this.control.person_gives_back(),
                    Ends::Rest => {
                        if let Some(agent) = this.control.agent().cloned() {
                            this.control.agent_releases(&agent);
                        }
                    }
                }
                cx.notify();
            });
        });
        *self.end(which) = Some((until, task));
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
            let control = &self.desk.read(cx).control;
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
            // A desktop the person is only using goes back by itself once they stop.
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
