//! Who drives a device (`docs/devices.md` 8): the lease a browser page has (`browser::control`),
//! one per device, with the agent desktop's timers. The person pre-empts any agent: by acting in
//! the tab, which holds the device while they're at it and [`IMPLICIT_HOLD`] after; or with Take
//! control, which holds it until they give it back (a simulator's: a device of the person's own
//! that is lent has Take it back, which ends the lend). Agents share a device as they share the
//! desktop, and one that rests ten seconds has let it go. On a real device the person can also
//! touch the glass, which no lease stops.

use std::time::{Duration, Instant};

use gpui_kit::Context;

use super::link::Link;
use crate::browser::control::{Actor, Control, IMPLICIT_HOLD, Mode, Refused};

/// How long after its last act an agent still has the device.
pub const RESTS_AFTER: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Hold,
    Rest,
}

impl Link {
    /// The person acts in the tab, or asks for the device: theirs from now on, and for
    /// [`IMPLICIT_HOLD`] past this act unless they asked for it.
    pub fn person_acts(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let held = self.control.holder.clone();
        if let Some(displaced) = self.control.person_takes(mode) {
            log::info!("the person took {} from {}", self.name, displaced.label);
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

    /// The person gives the device back: an agent may act again.
    pub fn give_back(&mut self, cx: &mut Context<Self>) {
        self.lapse = None;
        self.control.person_gives_back();
        cx.notify();
    }

    /// An agent is about to act: refused while the person holds the device, its own otherwise
    /// until it has rested [`RESTS_AFTER`].
    pub fn agent_acts(&mut self, actor: Actor, cx: &mut Context<Self>) -> Result<(), Refused> {
        let held = self.control.holder.clone();
        self.control.agent_claims(actor, true, None)?;
        self.ends(Ends::Rest, RESTS_AFTER, cx);
        if self.control.holder != held {
            cx.notify();
        }
        Ok(())
    }

    /// Nobody holds a device whose host went.
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

    /// Has `which` end `after` from now; one task waits on each, its deadline moved on.
    fn ends(&mut self, which: Ends, after: Duration, cx: &mut Context<Self>) {
        let until = cx.background_executor().now() + after;
        if let Some((deadline, _)) = self.end(which) {
            *deadline = until;
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                let left = this.update(cx, |this, cx| {
                    let (until, _) = this.end(which).as_ref()?;
                    Some(until.saturating_duration_since(cx.background_executor().now()))
                });
                match left {
                    Ok(Some(left)) if !left.is_zero() => cx.background_executor().timer(left).await,
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
