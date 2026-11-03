//! Who may write to the terminal (`docs/terminals.md` 5.6, D11). A person may always; their
//! input takes the terminal from an agent that holds it, and from then on agents stay out
//! until they give it back. An agent writes under a lease it claims, renews by acting, and
//! loses when it lapses, its connection closes, or another agent takes over.

use koyo_term_control::{
    Actor, ActorKind, AttachMode, ControlChange, Controller, ErrorCode, ErrorDetails, EventKind,
    ProtocolError, now_ms,
};

use super::Session;

/// How long an agent's lease lasts after it last acted.
pub const LEASE_MS: u64 = 120_000;

/// Input that only reports what the person's view did, a focus change (`CSI I`, `CSI O`) or
/// a wheel turn in a program tracking the mouse (SGR buttons 64 to 67), and so takes nothing:
/// looking at a terminal or scrolling it isn't typing in it (7.3).
pub fn is_passive(mut bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    while !bytes.is_empty() {
        if let Some(rest) =
            (bytes.strip_prefix(b"\x1b[I")).or_else(|| bytes.strip_prefix(b"\x1b[O"))
        {
            bytes = rest;
        } else if let Some(report) = bytes.strip_prefix(b"\x1b[<") {
            let Some(end) = report.iter().position(|&b| b == b'M' || b == b'm') else {
                return false;
            };
            let fields = &report[..end];
            let button = (fields.split(|&b| b == b';').next())
                .and_then(|field| std::str::from_utf8(field).ok()?.parse::<u16>().ok());
            // Shift, alt and ctrl add 4, 8 and 16 to the button.
            if !button.is_some_and(|button| (64..=67).contains(&(button & !0b1_1100))) {
                return false;
            }
            bytes = &report[end + 1..];
        } else {
            return false;
        }
    }
    true
}

impl Session {
    /// Whether `actor` may write to the terminal now: a person always may, taking it from an
    /// agent that holds it; an agent under its own lease, which acting renews.
    pub(super) fn may_act(&mut self, actor: &Actor) -> Result<(), ProtocolError> {
        self.expire_lease();
        match actor.kind {
            ActorKind::Human => {
                self.human_takes(actor, true);
                Ok(())
            }
            ActorKind::System => Ok(()),
            ActorKind::Agent => self.renew(actor),
        }
    }

    /// A person's claim is the explicit takeover; an agent's takes a free terminal, or with
    /// `takeover` another agent's, never a person's.
    pub(super) fn claim(
        &mut self,
        actor: &Actor,
        takeover: bool,
        reason: Option<String>,
    ) -> Result<(), ProtocolError> {
        self.expire_lease();
        if actor.kind != ActorKind::Agent {
            self.human_takes(actor, false);
            return Ok(());
        }
        let previous = self.record.control.holder.clone();
        match &previous {
            Some(Controller::Agent { actor: holder, .. }) if holder.id == actor.id => {
                return self.renew(actor);
            }
            Some(Controller::Human { .. }) => return Err(self.refusal(actor)),
            Some(Controller::Agent { .. }) if !takeover => return Err(self.refusal(actor)),
            _ => {}
        }
        let now = now_ms();
        let controller = Controller::Agent {
            actor: actor.clone(),
            since_ms: now,
            lease_until_ms: now + LEASE_MS,
            reason: reason.clone(),
        };
        self.taken.remove(&actor.id);
        let change = match previous {
            Some(Controller::Agent { actor: from, .. }) => {
                self.taken.insert(from.id.clone(), controller.clone());
                ControlChange::Taken {
                    by: actor.clone(),
                    from,
                    reason,
                }
            }
            _ => ControlChange::Claimed { by: actor.clone() },
        };
        self.set_control(Some(controller), change);
        Ok(())
    }

    /// Gives up what `actor` holds: a person's "Give back" opens the terminal to agents again.
    /// Releasing what the caller doesn't hold changes nothing.
    pub(super) fn release(&mut self, actor: &Actor) {
        self.taken.remove(&actor.id);
        let change = match &self.record.control.holder {
            Some(Controller::Human { .. }) if actor.kind == ActorKind::Human => {
                ControlChange::HumanReleased { by: actor.clone() }
            }
            Some(Controller::Agent { actor: holder, .. }) if holder.id == actor.id => {
                ControlChange::Released { by: actor.clone() }
            }
            _ => return,
        };
        self.set_control(None, change);
    }

    /// Extends the lease `actor` holds; refused, saying who holds the terminal, when it holds
    /// none. A person needs no lease.
    pub(super) fn renew(&mut self, actor: &Actor) -> Result<(), ProtocolError> {
        if actor.kind != ActorKind::Agent {
            return Ok(());
        }
        let Some(Controller::Agent {
            actor: holder,
            lease_until_ms,
            ..
        }) = &mut self.record.control.holder
        else {
            return Err(self.refusal(actor));
        };
        if holder.id != actor.id {
            return Err(self.refusal(actor));
        }
        let until = now_ms() + LEASE_MS;
        // Lists see the lease move a second at a time, not at every keystroke.
        let moved = until.saturating_sub(*lease_until_ms) >= 1000;
        *lease_until_ms = until;
        if moved {
            self.publish(false);
        }
        Ok(())
    }

    /// An agent's lease ends with the connection it was claimed on.
    pub(super) fn client_gone(&mut self, actor: &Actor) {
        if actor.kind != ActorKind::Agent {
            return;
        }
        self.taken.remove(&actor.id);
        if let Some(Controller::Agent { actor: holder, .. }) = &self.record.control.holder
            && holder.id == actor.id
        {
            let was = holder.clone();
            self.set_control(None, ControlChange::Expired { was });
        }
    }

    /// Lets a lease past its time go.
    pub(super) fn expire_lease(&mut self) {
        if let Some(Controller::Agent {
            actor,
            lease_until_ms,
            ..
        }) = &self.record.control.holder
            && *lease_until_ms <= now_ms()
        {
            let was = actor.clone();
            self.set_control(None, ControlChange::Expired { was });
        }
    }

    /// Who watches without writing: the clients attached to observe.
    pub(super) fn refresh_observers(&mut self) {
        let mut observers: Vec<Actor> = (self.clients.values())
            .filter(|attached| attached.mode == AttachMode::Observe)
            .map(|attached| attached.client.actor.clone())
            .collect();
        observers.sort_by(|a, b| a.id.cmp(&b.id));
        observers.dedup_by(|a, b| a.id == b.id);
        self.record.control.observers = observers;
    }

    /// A person takes the terminal: by asking, always; by typing (`implicit`), only from an
    /// agent, since typing in a terminal no one holds takes nothing.
    fn human_takes(&mut self, actor: &Actor, implicit: bool) {
        match &self.record.control.holder {
            Some(Controller::Human { .. }) => return,
            None if implicit => return,
            _ => {}
        }
        let controller = Controller::Human {
            actor: actor.clone(),
            since_ms: now_ms(),
            implicit,
        };
        let change = ControlChange::HumanTook {
            by: actor.clone(),
            implicit,
        };
        self.set_control(Some(controller), change);
    }

    /// Why `agent` may not write, naming who holds the terminal.
    fn refusal(&self, agent: &Actor) -> ProtocolError {
        let holder = self.record.control.holder.clone();
        let (code, message, holder) = match (holder, self.taken.get(&agent.id)) {
            (Some(holder @ Controller::Human { .. }), _) => (
                ErrorCode::PersonHasControl,
                "a person took this terminal over; leave it alone and open another in the \
                 worktree"
                    .to_owned(),
                Some(holder),
            ),
            (_, Some(taker)) => (
                ErrorCode::ControlTakenBy,
                format!(
                    "{} took control of this terminal; claim it again to go on",
                    taker.actor().label
                ),
                Some(taker.clone()),
            ),
            (Some(holder), None) => (
                ErrorCode::ControlRequired,
                format!(
                    "{} holds this terminal; claim it with takeover to displace it",
                    holder.actor().label
                ),
                Some(holder),
            ),
            (None, None) => (
                ErrorCode::ControlRequired,
                "claim the terminal before writing to it".to_owned(),
                None,
            ),
        };
        let error = ProtocolError::new(code, message);
        match holder {
            Some(holder) => error.with_details(ErrorDetails::Holder { holder }),
            None => error,
        }
    }

    fn set_control(&mut self, holder: Option<Controller>, change: ControlChange) {
        log::info!("{}: control {change:?}", self.record.handle);
        let from = (self.record.control.holder.as_ref()).map(|holder| holder.actor().clone());
        self.log_control(&change, from.as_ref());
        self.record.control.holder = holder;
        self.publish(false);
        self.daemon.broadcast(
            &self.record.handle,
            &EventKind::Control {
                change,
                control: self.record.control.clone(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::is_passive;

    #[test]
    fn focus_and_wheel_reports_are_passive_and_anything_else_is_typing() {
        assert!(is_passive(b"\x1b[I"));
        assert!(is_passive(b"\x1b[O\x1b[I"));
        assert!(is_passive(b"\x1b[<64;10;5M\x1b[<65;10;5M"));
        assert!(is_passive(b"\x1b[<80;3;4M"), "ctrl and wheel up");
        assert!(!is_passive(b"\x1b[<0;10;5M"), "a click");
        assert!(!is_passive(b"\x1b[Ia"), "a focus report and a key");
        assert!(!is_passive(b"\x1b[A"), "the up arrow");
        assert!(!is_passive(b""));
        assert!(!is_passive(b"\x1b[<64;10"), "cut short");
    }
}
