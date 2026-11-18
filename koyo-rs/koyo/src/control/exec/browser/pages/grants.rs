//! The consents the person gave for their pages while Koyo runs, in memory only, and how each
//! ends. A grant is one client's, on the connection it asked on, for one page: the page itself,
//! where it was, pinned at the address it had, and the lease the client took with it. It ends
//! as the client lets the page go, the person takes it (the client's lease is then gone, and
//! one taken later is another), 10 minutes go by without the client using it, the page closes,
//! moves, is pinned again or pinned at another address, the client's connection closes or its
//! chat ends, or the client may drive pages no more. A caller with no connection has none.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::{App, AsyncApp, Entity, EntityId, Global, Task, WeakEntity};
use koyo_agent::protocol::ids::ThreadId;
use koyo_app_control::Scope;

use super::super::super::super::gate;
use super::super::super::Caller;
use super::asks;
use crate::browser::agent::Actor;
use crate::browser::profiles::{self, By};
use crate::browser::BrowserView;
use crate::model::{Spot, Tab};
use crate::shell::Shell;

/// How long consent lasts without its client using the page.
pub(crate) const IDLE: Duration = Duration::from_mins(10);

/// A client's consent for one page, by the client's key and the page's pane.
pub(super) type Key = (String, u64);

/// Which page a consent is for: the page itself, so a pane that shows another later has none;
/// where it is, so a page moved or pinned again has none; and a pinned page's address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Which {
    pub view: Option<EntityId>,
    pub spot: Spot,
    pub home: Option<String>,
}

impl Which {
    /// Whether it's the page `other` is, where `other` is: a worktree's page by its checkout,
    /// not its place among the tabs, which tabs opened and closed before it move.
    fn is(&self, other: &Which) -> bool {
        let here = |spot: Spot| match spot {
            Spot::Checkout(project, branch, _) => Spot::Checkout(project, branch, 0),
            spot => spot,
        };
        self.view == other.view && self.home == other.home && here(self.spot) == here(other.spot)
    }
}

/// What the person allowed a client.
struct Grant {
    which: Which,
    view: WeakEntity<BrowserView>,
    /// The client, as it called when it asked.
    caller: Caller,
    actor: Actor,
    /// Which of the page's leases the client took with it.
    lease: u64,
    /// The connection it asked on.
    connection: u64,
    /// When it lapses unless the client uses the page before.
    until: Instant,
    /// Tells it from a grant made later for the same client and page.
    number: u64,
    _lapse: Task<()>,
}

#[derive(Default)]
struct Consents {
    grants: HashMap<Key, Grant>,
    /// The connections open now.
    open: HashSet<u64>,
    next: u64,
}

impl Global for Consents {}

/// The grant `key` names, `caller`'s on its connection; none for a caller with no connection.
fn mine<'a>(key: &Key, caller: &Caller, cx: &'a App) -> Option<&'a Grant> {
    let grant = cx.try_global::<Consents>()?.grants.get(key)?;
    (Some(grant.connection) == caller.connection).then_some(grant)
}

/// Whether `grant` holds for `view`: given for that page, on the lease its holder holds now.
fn on_lease(grant: &Grant, view: &Entity<BrowserView>, cx: &App) -> bool {
    grant.which.view == Some(view.entity_id())
        && view.read(cx).agent_lease() == Some((&grant.actor, grant.lease))
}

/// The number of `caller`'s grant for pane `pane`, while it holds for `view` as `which` finds
/// it now.
pub(super) fn held(
    caller: &Caller,
    pane: u64,
    which: &Which,
    view: &Entity<BrowserView>,
    cx: &App,
) -> Option<u64> {
    let grant = mine(&(caller.key(), pane), caller, cx)?;
    (grant.which.is(which) && on_lease(grant, view, cx)).then_some(grant.number)
}

/// Whether `caller`'s grant numbered `number` for pane `pane` still holds for `view`.
pub(super) fn current(
    caller: &Caller,
    pane: u64,
    number: u64,
    view: &Entity<BrowserView>,
    cx: &App,
) -> bool {
    mine(&(caller.key(), pane), caller, cx)
        .is_some_and(|grant| grant.number == number && on_lease(grant, view, cx))
}

/// The grant covering the lease an agent holds of `view` in pane `pane` now, if one does.
fn covering<'a>(pane: u64, view: &Entity<BrowserView>, cx: &'a App) -> Option<&'a Grant> {
    let (actor, _) = view.read(cx).agent_lease()?;
    let grants = &cx.try_global::<Consents>()?.grants;
    (grants.get(&(actor.id.clone(), pane))).filter(|grant| on_lease(grant, view, cx))
}

/// Whether a grant covers the lease an agent holds of `view` in pane `pane` now.
pub(super) fn covered(pane: u64, view: &Entity<BrowserView>, cx: &App) -> bool {
    covering(pane, view, cx).is_some()
}

/// Whether the lease of `view` in pane `pane` is `caller`'s, under its own grant.
pub(super) fn covered_mine(
    caller: &Caller,
    pane: u64,
    view: &Entity<BrowserView>,
    cx: &App,
) -> bool {
    (covering(pane, view, cx)).is_some_and(|grant| Some(grant.connection) == caller.connection)
}

/// Whether the lease of `view` in pane `pane` is held under a grant of another connection's of
/// a client of `caller`'s name: another holder, to `caller`.
pub(super) fn covered_elsewhere(
    caller: &Caller,
    pane: u64,
    view: &Entity<BrowserView>,
    cx: &App,
) -> bool {
    (covering(pane, view, cx)).is_some_and(|grant| {
        grant.caller.key() == caller.key() && Some(grant.connection) != caller.connection
    })
}

/// The client used the page: its consent lasts [`IDLE`] from now.
pub(super) fn used(key: &Key, cx: &mut App) {
    let now = cx.background_executor().now();
    if let Some(grant) = cx.default_global::<Consents>().grants.get_mut(key) {
        grant.until = now + IDLE;
    }
}

/// The person allowed `caller`, on its connection, the page `which` names, shown in pane
/// `pane`, on the lease `lease` it took of `view`. Nothing for a caller with no connection.
pub(super) fn grant(
    caller: &Caller,
    (pane, which): (u64, Which),
    view: &Entity<BrowserView>,
    lease: u64,
    cx: &mut App,
) {
    let Some(connection) = caller.connection else {
        return;
    };
    let key = (caller.key(), pane);
    let consents = cx.default_global::<Consents>();
    consents.next += 1;
    let number = consents.next;
    let until = cx.background_executor().now() + IDLE;
    let lapse = lapse(key.clone(), number, cx);
    let grant = Grant {
        which,
        view: view.downgrade(),
        caller: caller.clone(),
        actor: caller.actor(),
        lease,
        connection,
        until,
        number,
        _lapse: lapse,
    };
    cx.default_global::<Consents>().grants.insert(key, grant);
}

/// Waits until the grant numbered `number` lapses, however often its client uses the page
/// meanwhile, and ends it then; done once it's ended otherwise.
fn lapse(key: Key, number: u64, cx: &mut App) -> Task<()> {
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let left = cx.update(|cx| {
                let consents = cx.try_global::<Consents>()?;
                let grant = (consents.grants.get(&key)).filter(|grant| grant.number == number)?;
                Some(grant.until.saturating_duration_since(cx.background_executor().now()))
            });
            match left {
                Some(left) if !left.is_zero() => cx.background_executor().timer(left).await,
                Some(_) => return cx.update(|cx| end(&key, cx)),
                None => return,
            }
        }
    })
}

/// Ends the consent `key` names, if there is one, and the client's lease of the page with it.
fn end(key: &Key, cx: &mut App) {
    if !cx.has_global::<Consents>() {
        return;
    }
    let Some(grant) = cx.global_mut::<Consents>().grants.remove(key) else {
        return;
    };
    let Some(view) = grant.view.upgrade() else {
        return;
    };
    if view.read(cx).agent_holding() == Some(&grant.actor) {
        view.update(cx, |view, cx| view.agent_releases(&grant.actor, cx));
    }
}

/// Ends `caller`'s consent for pane `pane`, given on its connection; another connection's under
/// the same name stays. Whether there was one.
pub(super) fn end_mine(caller: &Caller, pane: u64, cx: &mut App) -> bool {
    let key = (caller.key(), pane);
    let mine = mine(&key, caller, cx).is_some();
    if mine {
        end(&key, cx);
    }
    mine
}

/// Ends the consent for pane `pane` another connection under `caller`'s name was given, as the
/// person gave the page to `caller` in its place.
pub(super) fn end_elsewhere(caller: &Caller, pane: u64, cx: &mut App) {
    let key = (caller.key(), pane);
    if mine(&key, caller, cx).is_none() {
        end(&key, cx);
    }
}

/// Whether `caller`'s connection is still open: an answer that comes after it closed grants
/// nothing, and a caller with no connection is never there.
pub(crate) fn still_open(caller: &Caller, cx: &App) -> bool {
    caller.connection.is_some_and(|connection| {
        (cx.try_global::<Consents>()).is_some_and(|consents| consents.open.contains(&connection))
    })
}

/// A welcomed client's connection `connection` opened.
pub(crate) fn joined(connection: u64, cx: &mut App) {
    cx.default_global::<Consents>().open.insert(connection);
}

/// Connection `connection` closed: the consents given on it end.
pub(crate) fn left(connection: u64, cx: &mut App) {
    let consents = cx.default_global::<Consents>();
    consents.open.remove(&connection);
    let ended: Vec<Key> = (consents.grants.iter())
        .filter(|(_, grant)| grant.connection == connection)
        .map(|(key, _)| key.clone())
        .collect();
    for key in ended {
        end(&key, cx);
    }
}

/// The control policy changed: consents of clients that may no longer drive pages end.
pub(crate) fn rescope(cx: &mut App) {
    let Some(consents) = cx.try_global::<Consents>() else {
        return;
    };
    let ended: Vec<Key> = (consents.grants.iter())
        .filter(|(_, grant)| {
            !gate::grant(&grant.caller, cx).is_ok_and(|grant| grant.contains(&Scope::Browser))
        })
        .map(|(key, _)| key.clone())
        .collect();
    for key in ended {
        end(&key, cx);
    }
}

/// The window changed: consents for a page that closed, moved, was pinned again or at another
/// address, or whose chat ended, end, and those for a worktree's page once agents' pages are
/// signed in as the person again; a lease an agent holds of one of the person's pages that no
/// consent covers, as of a page pinned while an agent drove it, or one they opened in a
/// worktree as agents' pages were kept apart, is let go; and refusals that are over are
/// forgotten.
pub(crate) fn prune(shell: &Shell, cx: &mut App) {
    asks::forget_over(cx.background_executor().now(), cx);
    let workspace = &shell.workspace;
    let loose = (workspace.loose_by_space())
        .flat_map(|(_, pages, _)| pages.iter().map(|page| &page.tab));
    let separate = profiles::get(cx).separate();
    let worktrees = (workspace.projects.iter())
        .flat_map(|project| &project.branches)
        .flat_map(|unit| &unit.tabs)
        .flat_map(Tab::panes)
        .filter(|pane| separate && pane.by() == By::Person);
    let theirs = (workspace.pinned().iter().map(|pinned| &pinned.tab))
        .chain(loose)
        .flat_map(Tab::panes)
        .chain(worktrees)
        .filter_map(|pane| Some((pane.id, pane.view()?.clone())));
    let stray: Vec<_> = theirs
        .filter(|(pane, view)| view.read(cx).agent_holding().is_some() && !covered(*pane, view, cx))
        .collect();
    let ended: Vec<Key> = (cx.try_global::<Consents>().into_iter())
        .flat_map(|consents| consents.grants.iter())
        .filter(|((_, pane), grant)| !still(shell, *pane, grant, cx))
        .map(|(key, _)| key.clone())
        .collect();
    for key in ended {
        end(&key, cx);
    }
    for (_, view) in stray {
        let actor = view.read(cx).agent_holding().cloned();
        if let Some(actor) = actor {
            view.update(cx, |view, cx| view.agent_releases(&actor, cx));
        }
    }
}

/// Whether `grant` still has its page, in pane `pane` as it was, and its chat.
fn still(shell: &Shell, pane: u64, grant: &Grant, cx: &App) -> bool {
    let workspace = &shell.workspace;
    let view = (workspace.pane(pane))
        .and_then(|pane| pane.view())
        .map(Entity::entity_id);
    let home = (workspace.pinned_of(pane)).map(|pinned| pinned.home.clone());
    let there = workspace.spot_of(pane).map(|spot| Which { view, spot, home });
    let chat = (grant.caller.chat.as_ref()).is_none_or(|id| {
        let chats = shell.chats().read(cx);
        chats.knows(&ThreadId(id.clone())) != Some(false)
    });
    let theirs = !matches!(grant.which.spot, Spot::Checkout(..)) || profiles::get(cx).separate();
    there.is_some_and(|there| there.is(&grant.which)) && chat && theirs
}
