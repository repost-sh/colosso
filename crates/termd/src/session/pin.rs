//! A size pinned on the terminal for every viewer (`docs/responsive.md` 3.1): set by the app
//! for the person or by a relay for an agent, it wins over the clients' sizes until it's
//! unpinned, replaced, or the connection that set it goes. An agent's pin is a resize under
//! its lease and is logged as one; a person's takes nothing from an agent.

use colosso_term_control::{
    Actor, ActorKind, ErrorCode, Pinned, ProtocolError, ResponseBody, pin_cells_error,
};

use super::Session;
use super::cmd::{Reply, exited};

impl Session {
    pub(super) fn pin(&mut self, reply: &Reply, cells: Option<(u16, u16)>) {
        if let Some((cols, rows)) = cells {
            if let Some(message) = pin_cells_error(cols, rows) {
                return reply.err(ProtocolError::new(ErrorCode::InvalidArgument, message));
            }
            if self.pty.is_none() {
                return reply.err(exited());
            }
        }
        let actor = reply.client.actor.clone();
        if let Err(error) = self.may_resize(&actor, cells) {
            return reply.err(error);
        }
        let pinned = cells.map(|(cols, rows)| Pinned {
            cols,
            rows,
            by: actor,
        });
        let news = pinned != self.record.pinned;
        // A pin set or cleared is worth the log; a pin moved, as a grip drags it, isn't.
        let level = if pinned.is_some() == self.record.pinned.is_some() {
            log::Level::Debug
        } else {
            log::Level::Info
        };
        log::log!(level, "{}: pinned at {pinned:?}", self.record.handle);
        self.record.pinned = pinned;
        self.pinned_by = cells.map(|_| reply.client.id);
        self.settle_size(news);
        reply.ok(ResponseBody::Pinned {
            size: self.record.size,
            pinned: self.record.pinned.clone(),
        });
    }

    /// The connection `client_id` closed: the pin it set goes, and the clients size the
    /// terminal again.
    pub(super) fn pin_gone(&mut self, client_id: u32) {
        if self.pinned_by != Some(client_id) {
            return;
        }
        log::info!(
            "{}: the pin went with client {client_id}",
            self.record.handle
        );
        self.pinned_by = None;
        self.record.pinned = None;
        self.settle_size(true);
    }

    /// Whether `actor` may size the terminal to `cells` (`None`: to fill the views), an
    /// agent's sizing logged as its resize; a person's sizes it as a window does, which
    /// isn't news.
    pub(super) fn may_resize(
        &mut self,
        actor: &Actor,
        cells: Option<(u16, u16)>,
    ) -> Result<(), ProtocolError> {
        let outcome = self.agent_may_act(actor);
        if actor.kind == ActorKind::Agent {
            let (command, action) = match cells {
                Some((cols, rows)) => (
                    format!("resize {cols}x{rows}"),
                    format!("resized to {cols}x{rows}"),
                ),
                None => (
                    "resize --fill".to_owned(),
                    "let it fill the views again".to_owned(),
                ),
            };
            self.log(actor, command, action, outcome.as_ref().copied());
        }
        outcome
    }
}
