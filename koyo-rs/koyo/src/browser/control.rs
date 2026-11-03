//! Who drives a page: a lease one controller holds at a time, the person or an agent, as a
//! terminal's is (`docs/terminals.md`, 5.6). Reads never need it; acting does. The person
//! pre-empts any agent: implicitly, with a click or key they send the page, which holds the
//! page only while they're at it, the page going back [`IMPLICIT_HOLD`] after their last
//! act; or explicitly, with Take control, which holds it, agents refused, until they give
//! it back. Unlike a terminal's, then, a page taken by using it comes back on its own: a
//! click on a page an agent drives is a pause, not a takeover.

use std::time::{Duration, Instant};

use gpui_kit::Context;

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
        match &self.holder {
            Some(Holder::Human { since, mode }) => {
                return Err(Refused::PersonHasControl {
                    since: *since,
                    mode: *mode,
                });
            }
            Some(Holder::Agent { actor: holder, .. }) if *holder != actor && !takeover => {
                return Err(Refused::ControlRequired {
                    holder: holder.clone(),
                });
            }
            Some(Holder::Agent {
                actor: holder,
                since,
                ..
            }) if *holder == actor => {
                // Its own lease, renewed.
                self.holder = Some(Holder::Agent {
                    actor,
                    since: *since,
                    reason,
                });
                return Ok(());
            }
            _ => {}
        }
        self.holder = Some(Holder::Agent {
            actor,
            since: Instant::now(),
            reason,
        });
        self.displaced = None;
        Ok(())
    }

    /// The person acts in the page or asks for it: theirs from now on. The agent that
    /// drove it, if one did, is displaced, and named until the person gives the page back.
    pub fn person_takes(&mut self, mode: Mode) -> Option<Actor> {
        let displaced = match self.holder.take() {
            Some(Holder::Human { since, mode: held }) => {
                // Already theirs. Asked for, a page they were only using is theirs to keep.
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
}

impl BrowserView {
    /// The person acts in the page: theirs from now on, whoever drove it, and for
    /// [`IMPLICIT_HOLD`] past this act unless they asked for it.
    pub(super) fn person_acts(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let held = self.control.holder.clone();
        if let Some(displaced) = self.control.person_takes(mode) {
            log::info!("the person took a browser page from {}", displaced.label);
        }
        self.keep_hold(cx);
        if self.control.holder != held {
            cx.notify();
        }
    }

    /// The person is at the page. Taken by acting in it, it goes back [`IMPLICIT_HOLD`]
    /// from now unless they act again first; taken by asking, it stays theirs. One task
    /// waits on the hold however often they act: each act only moves its deadline on.
    pub(super) fn keep_hold(&mut self, cx: &mut Context<Self>) {
        if !self.control.person_using() {
            self.lapse = None;
            return;
        }
        let until = cx.background_executor().now() + IMPLICIT_HOLD;
        if let Some((deadline, _)) = &mut self.lapse {
            *deadline = until;
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            // Sleeps to the deadline, and on to it again each time an act moved it.
            loop {
                let left = this.read_with(cx, |this, cx| {
                    let (until, _) = this.lapse.as_ref()?;
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
            let _ = this.update(cx, BrowserView::give_back);
        });
        self.lapse = Some((until, task));
    }

    /// The person gives the page back, or their hold of it lapses: an agent may drive it
    /// again.
    pub(super) fn give_back(&mut self, cx: &mut Context<Self>) {
        self.lapse = None;
        self.control.person_gives_back();
        cx.notify();
    }

    /// `actor`, an agent, asks to drive the page.
    pub fn agent_claims(
        &mut self,
        actor: Actor,
        takeover: bool,
        reason: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), Refused> {
        let claimed = self.control.agent_claims(actor, takeover, reason);
        cx.notify();
        claimed
    }

    /// `actor` is done with the page.
    pub fn agent_releases(&mut self, actor: &Actor, cx: &mut Context<Self>) {
        self.control.agent_releases(actor);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str) -> Actor {
        Actor {
            id: id.into(),
            label: id.to_uppercase(),
        }
    }

    #[test]
    fn an_agent_claims_a_free_page_and_takes_over_another_agents_only_by_saying_so() {
        let mut control = Control::default();
        assert_eq!(control.agent_claims(agent("a"), false, None), Ok(()));
        assert_eq!(control.agent(), Some(&agent("a")));
        assert_eq!(
            control.agent_claims(agent("b"), false, None),
            Err(Refused::ControlRequired { holder: agent("a") })
        );
        assert_eq!(
            control.agent_claims(agent("a"), false, None),
            Ok(()),
            "renewed"
        );
        assert_eq!(
            control.agent_claims(agent("b"), true, Some("rerunning".into())),
            Ok(())
        );
        assert_eq!(control.agent(), Some(&agent("b")));
        control.agent_releases(&agent("a"));
        assert_eq!(control.agent(), Some(&agent("b")), "not its lease to end");
        control.agent_releases(&agent("b"));
        assert_eq!(control.holder, None);
    }

    #[test]
    fn the_person_takes_a_page_and_agents_stay_out_until_they_give_it_back() {
        let mut control = Control::default();
        control.agent_claims(agent("a"), false, None).ok();
        assert_eq!(control.person_takes(Mode::Implicit), Some(agent("a")));
        assert!(control.person_holds());
        assert_eq!(control.displaced, Some(agent("a")));
        assert!(matches!(
            control.agent_claims(agent("a"), true, None),
            Err(Refused::PersonHasControl {
                mode: Mode::Implicit,
                ..
            })
        ));
        assert_eq!(control.person_takes(Mode::Explicit), None, "already theirs");
        assert!(
            matches!(
                control.agent_claims(agent("a"), true, None),
                Err(Refused::PersonHasControl {
                    mode: Mode::Explicit,
                    ..
                })
            ),
            "asked for, a page they were using is theirs to keep"
        );
        assert_eq!(control.person_takes(Mode::Implicit), None);
        assert!(
            !control.person_using(),
            "and using it doesn't make it a page they're only using"
        );
        assert!(matches!(
            control.agent_claims(agent("b"), false, None),
            Err(Refused::PersonHasControl { .. })
        ));
        control.person_gives_back();
        assert_eq!(control.holder, None);
        assert_eq!(control.displaced, None);
        assert_eq!(control.agent_claims(agent("b"), false, None), Ok(()));
    }

    #[test]
    fn a_page_nobody_held_is_the_persons_once_they_use_it() {
        let mut control = Control::default();
        assert_eq!(control.person_takes(Mode::Implicit), None);
        assert!(control.person_holds());
        assert!(control.person_using());
        assert_eq!(control.displaced, None, "nobody to name");
        assert!(control.agent_claims(agent("a"), false, None).is_err());
    }
}
