//! The gate the person's own pages, pinned and loose (`docs/app-control.md` 5, Browser), go
//! through. One of their pages reaches a tool only as [`Persons`], which hands the page over
//! only to a client the person let use it, on the connection it asked on and the lease it took
//! then, while it may still drive pages ([`Persons::admit`]); the others say
//! `consent_required`. A call let through is [`Admitted`], which its answer is checked against
//! once it's done, and before each command it sends (`Page::still`, `Page::settle`).

use gpui_kit::{App, Entity, WeakEntity};
use koyo_app_control::{ControlError, Scope};

use super::super::super::super::gate;
use super::super::super::Caller;
use super::super::page::refused;
use super::grants::{self, Which};
use super::{Admission, Page, asks};
use crate::browser::BrowserView;
use crate::browser::agent::Refused;
use crate::model::Spot;

/// One of the person's own pages a call names, as a tool has it before the gate: what it is,
/// but not the page, which only [`Persons::admit`] hands over. It holds the page weakly, so a
/// question waiting on the person keeps nothing of it alive.
pub(in super::super) struct Persons {
    pub pane: u64,
    pub spot: Spot,
    pub url: String,
    pub title: String,
    /// The address it's pinned at, a pinned page's.
    home: Option<String>,
    view: Option<WeakEntity<BrowserView>>,
}

/// What a call on the person's page was let through on: its caller, and the grant.
#[derive(Clone)]
pub(super) struct Admitted {
    caller: Caller,
    number: u64,
    kind: &'static str,
}

/// Why `pane`'s page, the person's `kind` page, isn't the caller's to use.
fn unconsented(pane: u64, kind: &str) -> ControlError {
    ControlError::consent_required(format!(
        "Pane {pane} is the person's {kind} page. Call browser_claim with pane {pane} to ask them, or browser_duplicate to ask for a copy in your worktree."
    ))
}

/// A refusal of the person's page's lease, which names no other client: another holds it, which
/// takeover asks the person for.
fn refused_theirs(why: Refused) -> ControlError {
    match why {
        Refused::ControlRequired { .. } => another_holds(),
        why @ Refused::PersonHasControl { .. } => refused(why),
    }
}

fn another_holds() -> ControlError {
    ControlError::control_required(
        "Another agent holds this page; browser_claim with takeover asks the person for it.",
    )
}

/// Whether `caller` may drive pages now, as the person's consent for any of theirs needs.
fn may_drive(caller: &Caller, cx: &App) -> Result<(), ControlError> {
    if gate::grant(caller, cx)?.contains(&Scope::Browser) {
        Ok(())
    } else {
        Err(ControlError::scope_denied(Scope::Browser))
    }
}

impl Persons {
    pub(super) fn new(
        (pane, spot): (u64, Spot),
        (url, title): (String, String),
        home: Option<String>,
        view: Option<&Entity<BrowserView>>,
    ) -> Persons {
        Persons {
            pane,
            spot,
            url,
            title,
            home,
            view: view.map(Entity::downgrade),
        }
    }

    /// "pinned" or "loose".
    pub(in super::super) fn kind(&self) -> &'static str {
        match self.spot {
            Spot::Pinned => "pinned",
            Spot::Loose(_) | Spot::Checkout(..) => "loose",
        }
    }

    /// Which page it is, as a grant is for it.
    fn which(&self) -> Which {
        Which {
            view: self.view.as_ref().map(|view| view.entity_id()),
            spot: self.spot,
            home: self.home.clone(),
        }
    }

    /// Its page, while it's open.
    fn open(&self) -> Option<Entity<BrowserView>> {
        self.view.as_ref()?.upgrade()
    }

    /// The number of `caller`'s grant for the page, while it holds.
    fn held(&self, caller: &Caller, cx: &App) -> Option<u64> {
        let view = self.open()?;
        grants::held(caller, self.pane, &self.which(), &view, cx)
    }

    /// The gate: the page, for `caller` while the person lets it use it, its use noted; else
    /// `consent_required`, saying how to ask. A client that may no longer drive pages loses
    /// its consent.
    pub(in super::super) fn admit(
        self,
        caller: &Caller,
        cx: &mut App,
    ) -> Result<Page, ControlError> {
        if let Err(refused) = may_drive(caller, cx) {
            grants::end_mine(caller, self.pane, cx);
            return Err(refused);
        }
        let (Some(number), Some(view)) = (self.held(caller, cx), self.open()) else {
            grants::end_mine(caller, self.pane, cx);
            return Err(unconsented(self.pane, self.kind()));
        };
        grants::used(&(caller.key(), self.pane), cx);
        let admitted = Admitted {
            caller: caller.clone(),
            number,
            kind: self.kind(),
        };
        Ok(Page::new(self.pane, &view, Admission::Persons(admitted)))
    }

    /// Renews `caller`'s lease while the person lets it use the page: whether they do.
    pub(in super::super) fn renew(
        &self,
        caller: &Caller,
        (takeover, reason): (bool, Option<String>),
        cx: &mut App,
    ) -> Result<bool, ControlError> {
        let (Some(_), Some(view)) = (self.held(caller, cx), self.open()) else {
            return Ok(false);
        };
        let actor = caller.actor();
        (view.update(cx, |view, cx| view.agent_claims(actor, takeover, reason, cx)))
            .map_err(refused_theirs)?;
        grants::used(&(caller.key(), self.pane), cx);
        Ok(true)
    }

    /// Whether the person may be asked to let `caller` use the page: it may ask about the page
    /// now (`asks.rs`), it has a connection to bind the consent to, the page is open, and nobody
    /// holds it whom a claim wouldn't take it from (a connection of the same name's, under its
    /// own consent, being another).
    pub(in super::super) fn may_ask(
        &self,
        caller: &Caller,
        takeover: bool,
        cx: &App,
    ) -> Result<(), ControlError> {
        asks::may_ask(caller, self.pane, cx)?;
        if !grants::still_open(caller, cx) {
            return Err(ControlError::invalid_input(
                "This call has no connection of Koyo's to bind the person's consent to; call through Koyo's MCP server or control socket.",
            ));
        }
        let view = self.open().ok_or_else(|| self.closed())?;
        self.may_take(caller, &view, takeover, cx)
    }

    /// Whether `caller` may take the page's lease, with `takeover` or not.
    fn may_take(
        &self,
        caller: &Caller,
        view: &Entity<BrowserView>,
        takeover: bool,
        cx: &App,
    ) -> Result<(), ControlError> {
        let actor = caller.actor();
        (view.read(cx).agent_may_claim(&actor, takeover)).map_err(refused_theirs)?;
        if !takeover && grants::covered_elsewhere(caller, self.pane, view, cx) {
            return Err(another_holds());
        }
        Ok(())
    }

    /// Why a page with none open, a pinned page closed or one not shown since launch, can't be
    /// used: it opens only as the person picks it.
    fn closed(&self) -> ControlError {
        ControlError::browser_unavailable(format!(
            "Pane {} is the person's {} page, and it isn't open: it opens once they pick it in the sidebar, which is theirs to do. browser_duplicate asks for a copy at its address in your worktree.",
            self.pane,
            self.kind()
        ))
        .retryable(false)
    }

    /// Whether it's the page `other` was: the same pane, showing the same page, where it was,
    /// pinned at the same address.
    pub(in super::super) fn same(&self, other: &Persons) -> bool {
        (self.pane, self.which()) == (other.pane, other.which())
    }

    /// The person allowed `caller` the page: it takes the page's lease, and may use the page
    /// until the consent ends. Refused once its connection has closed.
    pub(in super::super) fn grant(
        &self,
        caller: &Caller,
        (takeover, reason): (bool, Option<String>),
        cx: &mut App,
    ) -> Result<(), ControlError> {
        if !grants::still_open(caller, cx) {
            return Err(gone());
        }
        let view = self.open().ok_or_else(|| self.closed())?;
        self.may_take(caller, &view, takeover, cx)?;
        // Taken from a connection of the same name, the page is the caller's on a lease of its
        // own: the other's consent ends, its lease with it.
        grants::end_elsewhere(caller, self.pane, cx);
        let actor = caller.actor();
        (view.update(cx, |view, cx| view.agent_claims(actor, takeover, reason, cx)))
            .map_err(refused_theirs)?;
        let lease = view.read(cx).agent_lease().map(|(_, lease)| lease);
        let lease = lease.ok_or_else(|| ControlError::internal("The page's lease went at once."))?;
        grants::grant(caller, (self.pane, self.which()), &view, lease, cx);
        Ok(())
    }

    /// `caller` lets the page go: the consent given on its connection, and its lease with it.
    /// A lease held under another connection's consent, of the same name, is that one's to let
    /// go, and stays.
    pub(in super::super) fn release(&self, caller: &Caller, cx: &mut App) {
        if grants::end_mine(caller, self.pane, cx) {
            return;
        }
        if let Some(view) = self.open()
            && !grants::covered(self.pane, &view, cx)
        {
            let actor = caller.actor();
            view.update(cx, |view, cx| view.agent_releases(&actor, cx));
        }
    }

    /// Whether the person lets `caller` use the page now, as `browser_list` says.
    pub(super) fn granted(&self, caller: &Caller, cx: &App) -> bool {
        self.held(caller, cx).is_some() && may_drive(caller, cx).is_ok()
    }
}

/// Why the person's answer grants nothing: the connection that asked had closed by then.
pub(in super::super) fn gone() -> ControlError {
    ControlError::not_found(
        "The person allowed it, but the connection that asked had closed by then, so nothing was granted; ask again.",
    )
}

impl Admitted {
    /// Whether the person still lets the call's caller use `page`: the grant it was let through
    /// on holds, on the lease it was given with, and the caller may still drive pages.
    pub(super) fn holds(&self, page: &Page, cx: &App) -> Result<(), ControlError> {
        let current = (page.view.upgrade())
            .is_some_and(|view| grants::current(&self.caller, page.pane, self.number, &view, cx));
        if !current {
            return Err(unconsented(page.pane, self.kind));
        }
        may_drive(&self.caller, cx)
    }

    /// The client the call was let through for.
    pub(super) fn caller(&self) -> &Caller {
        &self.caller
    }
}
