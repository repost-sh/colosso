//! Who drives a page: a lease one controller holds at a time, the person or an agent, as a
//! terminal's is (`docs/terminals.md`, 5.6). Reads never need it; acting does. The person
//! pre-empts any agent: implicitly, with a click or key they send the page, which holds the
//! page only while they're at it, the page going back [`IMPLICIT_HOLD`] after their last
//! act; or explicitly, with Take control, which holds it, agents refused, until they give
//! it back. Unlike a terminal's, then, a page taken by using it comes back on its own: a
//! click on a page an agent drives is a pause, not a takeover. A device and the agent desktop
//! hold the same lease ([`Leased`]) as a page, where an agent's hold also lapses once it rests.

use std::time::{Duration, Instant};

use gpui_kit::{AsyncApp, Context, Task, WeakEntity};

use super::view::BrowserView;

/// How long a page taken by acting in it stays the person's past their last act.
pub const IMPLICIT_HOLD: Duration = Duration::from_secs(5);

/// An agent, by the id the control layer keeps its calls under and the name the person
/// knows it by, such as `claude-code:7f3a`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub id: String,
    pub label: String,
}

/// How the person took a page: by acting in it, or by asking for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Implicit,
    Explicit,
}

/// Who holds a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    Human {
        since: Instant,
        mode: Mode,
    },
    Agent {
        actor: Actor,
        since: Instant,
        reason: Option<String>,
    },
}

/// Why an agent may not act on a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The person holds the page: agents stay out until they give it back, which a page
    /// they're only using is on its own once they've left it be ([`IMPLICIT_HOLD`]), and a
    /// page they asked for is when they say so.
    PersonHasControl { since: Instant, mode: Mode },
    /// Another agent holds the page; a claim with takeover would succeed.
    ControlRequired { holder: Actor },
}

/// A page's lease.
#[derive(Debug, Default)]
pub struct Control {
    pub holder: Option<Holder>,
    /// The agent the person took the page from, named until they give it back.
    pub displaced: Option<Actor>,
    /// Counts the agents' leases the page has had: each new one gets the next, a renewal keeps
    /// its own, so a lease is told from one taken after it by the same agent.
    leases: u64,
}

impl Control {
    /// `actor` asks to drive the page: granted with nobody holding it or its own lease,
    /// with `takeover` over another agent's, never over the person's.
    pub fn agent_claims(
        &mut self,
        actor: Actor,
        takeover: bool,
        reason: Option<String>,
    ) -> Result<(), Refused> {
        self.may_claim(&actor, takeover)?;
        if let Some(Holder::Agent {
            actor: holder,
            since,
            ..
        }) = &self.holder
            && *holder == actor
        {
            self.holder = Some(Holder::Agent {
                actor,
                since: *since,
                reason,
            });
            return Ok(());
        }
        self.holder = Some(Holder::Agent {
            actor,
            since: Instant::now(),
            reason,
        });
        self.leases += 1;
        self.displaced = None;
        Ok(())
    }

    /// Whether `actor` may drive the page, as [`Control::agent_claims`] would let it: never
    /// over the person, over another agent only with `takeover`.
    pub fn may_claim(&self, actor: &Actor, takeover: bool) -> Result<(), Refused> {
        match &self.holder {
            Some(Holder::Human { since, mode }) => Err(Refused::PersonHasControl {
                since: *since,
                mode: *mode,
            }),
            Some(Holder::Agent { actor: holder, .. }) if holder != actor && !takeover => {
                Err(Refused::ControlRequired {
                    holder: holder.clone(),
                })
            }
            _ => Ok(()),
        }
    }

    /// The person acts in the page or asks for it: theirs from now on. The agent that
    /// drove it, if one did, is displaced, and named until the person gives the page back.
    pub fn person_takes(&mut self, mode: Mode) -> Option<Actor> {
        let displaced = match self.holder.take() {
            Some(Holder::Human { since, mode: held }) => {
                let mode = if mode == Mode::Explicit { mode } else { held };
                self.holder = Some(Holder::Human { since, mode });
                return None;
            }
            Some(Holder::Agent { actor, .. }) => Some(actor),
            None => None,
        };
        self.holder = Some(Holder::Human {
            since: Instant::now(),
            mode,
        });
        self.displaced.clone_from(&displaced);
        displaced
    }

    /// The person gives the page back: nobody holds it, and an agent may claim it again.
    pub fn person_gives_back(&mut self) {
        if matches!(self.holder, Some(Holder::Human { .. })) {
            self.holder = None;
        }
        self.displaced = None;
    }

    /// `actor` is done with the page.
    pub fn agent_releases(&mut self, actor: &Actor) {
        if matches!(&self.holder, Some(Holder::Agent { actor: holder, .. }) if holder == actor) {
            self.holder = None;
        }
    }

    pub fn person_holds(&self) -> bool {
        matches!(self.holder, Some(Holder::Human { .. }))
    }

    /// Whether the person holds the page by only using it: theirs while they're at it.
    pub fn person_using(&self) -> bool {
        matches!(
            self.holder,
            Some(Holder::Human {
                mode: Mode::Implicit,
                ..
            })
        )
    }

    /// The agent driving the page, if one does.
    pub fn agent(&self) -> Option<&Actor> {
        match &self.holder {
            Some(Holder::Agent { actor, .. }) => Some(actor),
            _ => None,
        }
    }

    /// The agent driving the page and which of the page's leases it holds, if one does.
    pub fn agent_lease(&self) -> Option<(&Actor, u64)> {
        self.agent().map(|actor| (actor, self.leases))
    }
}

/// How long after its last act an agent still has a device or the agent desktop.
pub const RESTS_AFTER: Duration = Duration::from_secs(10);

/// What comes to its end by itself: the person's hold by acting, an agent's by resting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Hold,
    Rest,
}

/// The lease of a page, a device or the agent desktop, and a task for each hold that ends by
/// itself: one task however often the hold is renewed, each renewal only moving its deadline
/// on.
#[derive(Default)]
pub struct Lease {
    pub control: Control,
    lapse: Option<(Instant, Task<()>)>,
    rest: Option<(Instant, Task<()>)>,
}

impl Lease {
    fn end(&mut self, which: Ends) -> &mut Option<(Instant, Task<()>)> {
        match which {
            Ends::Hold => &mut self.lapse,
            Ends::Rest => &mut self.rest,
        }
    }

    fn deadline(&self, which: Ends) -> Option<Instant> {
        let end = match which {
            Ends::Hold => &self.lapse,
            Ends::Rest => &self.rest,
        };
        end.as_ref().map(|(deadline, _)| *deadline)
    }

    /// `which` came to its end: the person's hold lapses, or the resting agent lets go.
    fn ended(&mut self, which: Ends) {
        *self.end(which) = None;
        match which {
            Ends::Hold => self.control.person_gives_back(),
            Ends::Rest => {
                if let Some(agent) = self.control.agent().cloned() {
                    self.control.agent_releases(&agent);
                }
            }
        }
    }
}

/// A page, a device or the agent desktop, which agents share and the person pre-empts, by its
/// [`Lease`]: the person by acting in its tab, which holds it while they're at it and
/// [`IMPLICIT_HOLD`] after, or with Take control, which holds it until they give it back. On a
/// device or the agent desktop, an agent's act takes it for that agent, from another agent too,
/// and an agent that rests [`RESTS_AFTER`] has let it go.
pub trait Leased: Sized + 'static {
    fn lease(&self) -> &Lease;
    fn lease_mut(&mut self) -> &mut Lease;
    /// What the log calls it.
    fn named(&self) -> &str;

    /// The person acts in the tab, or asks for it: theirs from now on, whoever drove it, and
    /// for [`IMPLICIT_HOLD`] past this act unless they asked for it.
    fn person_acts(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let held = self.lease().control.holder.clone();
        if let Some(displaced) = self.lease_mut().control.person_takes(mode) {
            log::info!("the person took {} from {}", self.named(), displaced.label);
        }
        if self.lease().control.person_using() {
            ends(self, Ends::Hold, IMPLICIT_HOLD, cx);
        } else {
            self.lease_mut().lapse = None;
        }
        if self.lease().control.holder != held {
            cx.notify();
        }
    }

    /// The person gives it back: nobody holds it, and an agent may act again.
    fn give_back(&mut self, cx: &mut Context<Self>) {
        let lease = self.lease_mut();
        lease.lapse = None;
        lease.control.person_gives_back();
        cx.notify();
    }

    /// An agent is about to act: refused while the person holds it, and its own otherwise,
    /// until it has rested [`RESTS_AFTER`].
    fn agent_acts(&mut self, actor: Actor, cx: &mut Context<Self>) -> Result<(), Refused> {
        let held = self.lease().control.holder.clone();
        self.lease_mut().control.agent_claims(actor, true, None)?;
        ends(self, Ends::Rest, RESTS_AFTER, cx);
        if self.lease().control.holder != held {
            cx.notify();
        }
        Ok(())
    }
}

/// Has `which` end `after` from now.
fn ends<T: Leased>(this: &mut T, which: Ends, after: Duration, cx: &mut Context<T>) {
    let until = cx.background_executor().now() + after;
    if let Some((deadline, _)) = this.lease_mut().end(which) {
        *deadline = until;
        return;
    }
    let task = cx.spawn(async move |this, cx| {
        if sleep_until(&this, |this: &T| this.lease().deadline(which), cx).await {
            let _ = this.update(cx, |this, cx| {
                this.lease_mut().ended(which);
                cx.notify();
            });
        }
    });
    *this.lease_mut().end(which) = Some((until, task));
}

impl Leased for BrowserView {
    fn lease(&self) -> &Lease {
        &self.lease
    }

    fn lease_mut(&mut self) -> &mut Lease {
        &mut self.lease
    }

    fn named(&self) -> &'static str {
        "a browser page"
    }
}

impl BrowserView {
    /// Whether the person has the page, as a test outside the browser reads it.
    #[cfg(test)]
    pub fn person_has_it(&self) -> bool {
        self.lease.control.person_holds()
    }

    /// The page given back, as a test outside the browser does it.
    #[cfg(test)]
    pub fn nobody_has_it(&mut self, cx: &mut Context<Self>) {
        self.give_back(cx);
    }

    /// `actor`, an agent, asks to drive the page; one it drives is on screen for the engine
    /// a while, drawn or not (`view/sight.rs`).
    pub fn agent_claims(
        &mut self,
        actor: Actor,
        takeover: bool,
        reason: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        let claimed = self.lease.control.agent_claims(actor, takeover, reason);
        if claimed.is_ok() {
            self.agent_uses(cx);
        }
        cx.notify();
        claimed
    }

    /// `actor` is done with the page.
    pub fn agent_releases(&mut self, actor: &Actor, cx: &mut Context<Self>) {
        self.lease.control.agent_releases(actor);
        cx.notify();
    }

    /// Whether `actor` may drive the page now, as [`BrowserView::agent_claims`] would let it.
    pub fn agent_may_claim(&self, actor: &Actor, takeover: bool) -> Result<(), Refused> {
        self.lease.control.may_claim(actor, takeover)
    }

    /// The agent driving the page, if one does.
    pub fn agent_holding(&self) -> Option<&Actor> {
        self.lease.control.agent()
    }

    /// The agent driving the page and which of the page's leases it holds: one taken after it
    /// ended, by the same agent too, is another.
    pub fn agent_lease(&self) -> Option<(&Actor, u64)> {
        self.lease.control.agent_lease()
    }

    /// Whether the person holds the page.
    pub fn person_holding(&self) -> bool {
        self.lease.control.person_holds()
    }
}

/// Sleeps until the deadline `deadline` reads from `entity` (a page, a device, the agent
/// desktop), and on to it again each time it moved meanwhile, so one task waits however often
/// it moves: whether it passed, rather than the deadline or the entity going first.
pub(super) async fn sleep_until<T: 'static>(
    entity: &WeakEntity<T>,
    deadline: impl Fn(&T) -> Option<Instant>,
    cx: &mut AsyncApp,
) -> bool {
    loop {
        let left = entity.read_with(cx, |entity, cx| {
            Some(deadline(entity)?.saturating_duration_since(cx.background_executor().now()))
        });
        match left {
            Ok(Some(left)) if !left.is_zero() => cx.background_executor().timer(left).await,
            Ok(Some(_)) => return true,
            Ok(None) | Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests;
