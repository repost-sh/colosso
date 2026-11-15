//! Who may ask the person about one of their pages, and when. A client they turned down for a
//! page, known by its name and, apart from that, by its connection, isn't let ask about that
//! page again for 10 minutes; anyone else waits a minute after any refusal of it; and the person
//! is asked about a page by one client at a time.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::{App, Global};
use koyo_app_control::{ControlError, ErrorCode};

use super::super::super::Caller;

/// How long a client the person turned down for a page isn't let ask about it again.
pub(crate) const DECLINED_FOR: Duration = Duration::from_mins(10);
/// How long anyone else waits to ask about a page after the person turned a request for it down.
pub(crate) const OTHERS_WAIT: Duration = Duration::from_mins(1);

/// A client as a refusal remembers it: by the name it gives, and by its connection, so a new
/// name on the same connection is remembered too.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Asker {
    Named(String),
    Connected(u64),
}

#[derive(Default)]
struct Asks {
    /// Until when each client turned down for a page, by the page's pane, isn't let ask again.
    declined: HashMap<(u64, Asker), Instant>,
    /// Until when anyone waits to ask about each page turned down, by its pane.
    turned_down: HashMap<u64, Instant>,
    /// The pages the person is being asked about now, by their panes.
    asking: HashSet<u64>,
}

impl Global for Asks {}

fn askers(caller: &Caller) -> impl Iterator<Item = Asker> {
    let named = Asker::Named(caller.key());
    std::iter::once(named).chain(caller.connection.map(Asker::Connected))
}

/// How long is left until `until`, if anything is.
fn left(until: Option<&Instant>, cx: &App) -> Option<Duration> {
    let left = until?.saturating_duration_since(cx.background_executor().now());
    (!left.is_zero()).then_some(left)
}

/// Whether `caller` may ask the person about pane `pane`'s page now: refused at once while a
/// refusal of it is recent, or while the person is being asked about it already.
pub(crate) fn may_ask(caller: &Caller, pane: u64, cx: &App) -> Result<(), ControlError> {
    let Some(asks) = cx.try_global::<Asks>() else {
        return Ok(());
    };
    let mine = askers(caller).find_map(|asker| left(asks.declined.get(&(pane, asker)), cx));
    if let Some(left) = mine {
        return Err(ControlError::user_declined(format!(
            "The person turned down your request for pane {pane}; Koyo doesn't ask them about it for you again for {} more minutes.",
            left.as_secs().div_ceil(60)
        )));
    }
    if let Some(left) = left(asks.turned_down.get(&pane), cx) {
        return Err(ControlError::user_declined(format!(
            "The person turned down a request for pane {pane} just now; Koyo doesn't ask them about it again for {} seconds.",
            left.as_secs().max(1)
        )));
    }
    if asks.asking.contains(&pane) {
        return Err(ControlError::new(
            ErrorCode::RequestConflict,
            format!("The person is being asked about pane {pane} already; try again once they've answered."),
        )
        .retryable(true));
    }
    Ok(())
}

/// The person is being asked about pane `pane`'s page from now on, or no longer.
pub(crate) fn asking(pane: u64, now: bool, cx: &mut App) {
    let asking = &mut cx.default_global::<Asks>().asking;
    if now {
        asking.insert(pane);
    } else {
        asking.remove(&pane);
    }
}

/// The person turned `caller` down for pane `pane`'s page: it isn't let ask about it again
/// for [`DECLINED_FOR`], anyone else for [`OTHERS_WAIT`]. Refusals that are over are forgotten.
pub(crate) fn declined(caller: &Caller, pane: u64, cx: &mut App) {
    let now = cx.background_executor().now();
    forget_over(now, cx);
    let asks = cx.default_global::<Asks>();
    for asker in askers(caller) {
        asks.declined.insert((pane, asker), now + DECLINED_FOR);
    }
    asks.turned_down.insert(pane, now + OTHERS_WAIT);
}

/// Forgets the refusals that are over by `now`.
pub(super) fn forget_over(now: Instant, cx: &mut App) {
    if let Some(asks) = cx.try_global::<Asks>()
        && (asks.declined.values().chain(asks.turned_down.values())).any(|until| *until <= now)
    {
        let asks = cx.global_mut::<Asks>();
        asks.declined.retain(|_, until| *until > now);
        asks.turned_down.retain(|_, until| *until > now);
    }
}

/// How many clients' refusals are kept, by name and by connection: those over are forgotten.
#[cfg(test)]
pub(crate) fn declines_kept(cx: &App) -> usize {
    cx.try_global::<Asks>().map_or(0, |asks| asks.declined.len())
}
