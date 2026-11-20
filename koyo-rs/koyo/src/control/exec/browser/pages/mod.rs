//! The browser pages the tools reach, and the one gate the person's own go through: every
//! worktree's page and the person's, pinned and loose, and the one a call means (`listing.rs`);
//! the consent the person gives a client for one of theirs (`consent.rs`), kept and ended as
//! `grants.rs` says; and who may ask them, and when (`asks.rs`). A [`Page`] is made only here,
//! so a tool has one of the person's pages only past the gate, and its call's answer stands only
//! while it may still use the page ([`Page::settle`]): a worktree's page that becomes the
//! person's meanwhile is theirs from then on.

mod asks;
mod consent;
mod grants;
mod listing;

use gpui_kit::{App, Entity, WeakEntity};
use koyo_app_control::ControlError;
use serde_json::Value;

use self::consent::Admitted;
#[cfg(test)]
pub(crate) use self::asks::{DECLINED_FOR, OTHERS_WAIT, declines_kept};
pub(super) use self::asks::{asking, declined, may_ask};
pub(super) use self::consent::{Persons, gone};
#[cfg(test)]
pub(crate) use self::grants::IDLE;
pub(crate) use self::grants::{joined, left, prune, rescope};
pub(super) use self::grants::still_open;
pub(crate) use self::listing::list;
pub(super) use self::listing::{Found, find};
use super::super::{Caller, main_shell};
use crate::browser::BrowserView;
use crate::browser::profiles::{self, By};
use crate::model::Spot;

/// What a call was let use a page on.
#[derive(Clone)]
enum Admission {
    /// A worktree's page, by `Caller`: it needs no consent while it's a worktree's.
    Checkout(Caller),
    /// The person's page, by their consent.
    Persons(Admitted),
}

/// A browser page a call may use: a worktree's, or one of the person's they let its caller use.
/// It holds the page weakly, as a call may outlast it.
#[derive(Clone)]
pub(super) struct Page {
    pane: u64,
    view: WeakEntity<BrowserView>,
    admitted: Admission,
}

impl Page {
    fn new(pane: u64, view: &Entity<BrowserView>, admitted: Admission) -> Page {
        Page {
            pane,
            view: view.downgrade(),
            admitted,
        }
    }

    pub(super) fn pane(&self) -> u64 {
        self.pane
    }

    /// Who the call is for.
    pub(super) fn caller(&self) -> &Caller {
        match &self.admitted {
            Admission::Checkout(caller) => caller,
            Admission::Persons(admitted) => admitted.caller(),
        }
    }

    /// Its page, while it's open.
    pub(super) fn view(&self) -> Result<Entity<BrowserView>, ControlError> {
        self.view.upgrade().ok_or_else(|| {
            ControlError::browser_unavailable(format!("Pane {}'s page closed.", self.pane))
                .retryable(false)
        })
    }

    /// Whether the call may go on: its page is still open, and still a worktree's, or, the
    /// person's, they still let its caller use it on the lease it was given with.
    pub(super) fn still(&self, cx: &App) -> Result<(), ControlError> {
        self.view()?;
        match &self.admitted {
            Admission::Checkout(_) => self.still_a_worktrees(cx),
            Admission::Persons(admitted) => admitted.holds(self, cx),
        }
    }

    /// Refused once the worktree's page the call began on is the person's: pinned, or among a
    /// space's loose pages, meanwhile; or, one they opened, as agents' pages were kept apart.
    fn still_a_worktrees(&self, cx: &App) -> Result<(), ControlError> {
        let workspace = main_shell(cx).map(|(_, shell)| &shell.read(cx).workspace);
        let spot = workspace.and_then(|workspace| workspace.spot_of(self.pane));
        let opened_by_person = || {
            let pane = workspace.and_then(|workspace| workspace.pane(self.pane));
            pane.is_some_and(|pane| pane.by() == By::Person)
        };
        let kind = match spot {
            Some(Spot::Pinned) => "pinned",
            Some(Spot::Loose(_)) => "loose",
            Some(Spot::Checkout(..)) if profiles::get(cx).separate() && opened_by_person() => {
                "worktree"
            }
            Some(Spot::Checkout(..)) | None => return Ok(()),
        };
        Err(ControlError::consent_required(format!(
            "Pane {} became the person's {kind} page while your call ran, so its answer was dropped. Call browser_claim with pane {} to ask them for it.",
            self.pane, self.pane
        )))
    }

    /// The call's `answer`, which stands only if its page is still open and the call may still
    /// use it once it's done: else it's dropped, and any lease the call left its caller on the
    /// person's page, which no consent covers, let go.
    pub(super) fn settle(
        &self,
        answer: Result<Value, ControlError>,
        cx: &mut App,
    ) -> Result<Value, ControlError> {
        let view = self.view()?;
        let ended = match self.still(cx) {
            Ok(()) => return answer,
            Err(ended) => ended,
        };
        let actor = self.caller().actor();
        let left = view.read(cx).agent_holding() == Some(&actor);
        if left && !grants::covered(self.pane, &view, cx) {
            view.update(cx, |view, cx| view.agent_releases(&actor, cx));
        }
        Err(ended)
    }
}
