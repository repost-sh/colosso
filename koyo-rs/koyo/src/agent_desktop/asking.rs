//! What the tools ask of the desktop (`docs/agent-desktop.md` 10): carried to the host and
//! waited for, at most as long as the tool allows.

use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::{App, Context, Task};
use koyo_desk_wire::link::{Answer, Ask, Refusal, RefusalCode, ToHost};

use super::{Desk, State, desk};
use crate::browser::control::{Actor, Refused};

/// Asks the agent desktop's host on behalf of a tool, and waits at most `limit` for its
/// answer ([`Desk::ask`]).
pub fn ask(cx: &mut App, ask: Ask, limit: Duration) -> Task<Result<Answer, Refusal>> {
    desk(cx).update(cx, |desk, cx| desk.ask(ask, limit, cx))
}

/// An agent is about to act on the desktop ([`Desk::agent_acts`]): refused while the person
/// holds it, as on a browser page they took.
pub fn agent_acts(cx: &mut App, actor: Actor) -> Result<(), Refused> {
    desk(cx).update(cx, |desk, cx| desk.agent_acts(actor, cx))
}

impl Desk {
    /// Why nothing can be asked of the desktop now.
    fn unavailable(&self) -> Option<Refusal> {
        let why = match &self.state {
            State::Running | State::Starting => return None,
            State::Off => {
                "The agent desktop is off: the person turns it on in Settings → Agent desktop."
                    .to_owned()
            }
            State::Failed(why) => why.clone(),
        };
        Some(Refusal::new(RefusalCode::NoDesktop, why))
    }

    /// Asks the host and waits for its answer, at most `limit`: a refusal at once when
    /// there's no host to ask, and one when the time runs out, the host then told to drop it.
    pub fn ask(
        &mut self,
        ask: Ask,
        limit: Duration,
        cx: &mut Context<Self>,
    ) -> Task<Result<Answer, Refusal>> {
        if let Some(refusal) = self.unavailable() {
            return Task::ready(Err(refusal));
        }
        let (answer, answered) = oneshot::channel();
        let request = self.next;
        self.next += 1;
        self.pending.insert(request, answer);
        self.tell(ToHost::Request { request, ask });
        let late = cx.background_executor().timer(limit);
        cx.spawn(
            async move |this, cx| match futures::future::select(answered, late).await {
                futures::future::Either::Left((Ok(result), _)) => result,
                futures::future::Either::Left((Err(_), _)) => Err(Refusal::new(
                    RefusalCode::NoDesktop,
                    "The agent desktop's host went away.",
                )),
                futures::future::Either::Right(((), _)) => {
                    let _ = this.update(cx, |this, _| {
                        this.pending.remove(&request);
                        this.tell(ToHost::Cancel { request });
                    });
                    Err(Refusal::new(
                        RefusalCode::TimedOut,
                        "The agent desktop didn't answer in time.",
                    ))
                }
            },
        )
    }
}
