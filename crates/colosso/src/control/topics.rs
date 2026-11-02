//! Topics a client follows (`docs/app-control.md` 7): the shell's and the settings' changes
//! mark the topics they may touch, and every 100 ms the marked topics are read again; those
//! whose reads came out different are told to their followers, which read them again
//! themselves. A follower whose connection has gone, or who may no longer read, stops
//! hearing.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::time::Duration;

use colosso_app_control::{ControlError, Scope, Topic};
use gpui_kit::{App, AsyncApp, Subscription};

use super::{Answer, Caller, Control, exec, gate};
use crate::settings;

/// How a follower hears of a change: false once it can't, as its connection has gone.
pub(crate) type Sink = Box<dyn Fn(Topic) -> bool + Send>;

/// How long changes gather before the topics they marked are read again.
pub(crate) const BATCH: Duration = Duration::from_millis(100);

/// A connection following topics.
struct Follower {
    connection: u64,
    caller: Caller,
    topics: BTreeSet<Topic>,
    sink: Sink,
}

/// The followers, and what their topics read last.
#[derive(Default)]
pub(crate) struct Hub {
    followers: Vec<Follower>,
    /// Topics a change touched since they were read last.
    dirty: BTreeSet<Topic>,
    /// Whether a read of the marked topics is due.
    flushing: bool,
    /// What each followed topic read last, hashed.
    seen: HashMap<Topic, u64>,
    /// Marks the topics as the window's shell changes.
    pub shell_observer: Option<Subscription>,
}

impl Hub {
    /// How many connections follow topics.
    #[cfg(test)]
    pub(crate) fn followers(&self) -> usize {
        self.followers.len()
    }
}

/// Has `connection` follow `topics` as well, each change told to `sink`.
pub(crate) fn subscribe(
    connection: u64,
    caller: &Caller,
    topics: &[Topic],
    sink: Sink,
    reply: Answer<()>,
    cx: &mut App,
) {
    match gate::grant(caller, cx) {
        Ok(grant) if grant.contains(&Topic::SCOPE) => {}
        Ok(_) => return reply(Err(ControlError::scope_denied(Topic::SCOPE))),
        Err(error) => return reply(Err(error)),
    }
    // What a newly followed topic reads now, so its first event is a change from now.
    for &topic in topics {
        if !Control::get(cx).hub.seen.contains_key(&topic) {
            let read = fingerprint(topic, cx);
            Control::get_mut(cx).hub.seen.insert(topic, read);
        }
    }
    let hub = &mut Control::get_mut(cx).hub;
    match (hub.followers.iter_mut()).find(|follower| follower.connection == connection) {
        Some(follower) => follower.topics.extend(topics),
        None => hub.followers.push(Follower {
            connection,
            caller: caller.clone(),
            topics: topics.iter().copied().collect(),
            sink,
        }),
    }
    reply(Ok(()));
}

/// `connection` has gone: it follows nothing any more.
pub(crate) fn closed(connection: u64, cx: &mut App) {
    let hub = &mut Control::get_mut(cx).hub;
    hub.followers
        .retain(|follower| follower.connection != connection);
    forget_unfollowed(hub);
}

/// Followers who may no longer read stop hearing.
pub(crate) fn drop_refused(cx: &mut App) {
    let followers = std::mem::take(&mut Control::get_mut(cx).hub.followers);
    let kept: Vec<Follower> = (followers.into_iter())
        .filter(|follower| {
            gate::grant(&follower.caller, cx).is_ok_and(|grant| grant.contains(&Scope::Read))
        })
        .collect();
    let hub = &mut Control::get_mut(cx).hub;
    hub.followers = kept;
    forget_unfollowed(hub);
}

fn forget_unfollowed(hub: &mut Hub) {
    let followed: BTreeSet<Topic> = (hub.followers.iter())
        .flat_map(|follower| follower.topics.iter().copied())
        .collect();
    hub.seen.retain(|topic, _| followed.contains(topic));
}

/// Something `topics` read may have changed: they're read again shortly, if anyone follows
/// them.
pub(crate) fn mark(topics: &[Topic], cx: &mut App) {
    if !cx.has_global::<Control>() {
        return;
    }
    let hub = &mut Control::get_mut(cx).hub;
    let followed = |topic: &&Topic| {
        hub.followers
            .iter()
            .any(|follower| follower.topics.contains(topic))
    };
    let marked: Vec<Topic> = topics.iter().filter(followed).copied().collect();
    hub.dirty.extend(marked);
    if hub.dirty.is_empty() || hub.flushing {
        return;
    }
    hub.flushing = true;
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(BATCH).await;
        cx.update(flush);
    })
    .detach();
}

/// Reads the marked topics again, and tells the followers of those that changed.
fn flush(cx: &mut App) {
    let hub = &mut Control::get_mut(cx).hub;
    hub.flushing = false;
    let dirty = std::mem::take(&mut hub.dirty);
    for topic in dirty {
        let read = fingerprint(topic, cx);
        let hub = &mut Control::get_mut(cx).hub;
        if hub.seen.get(&topic) == Some(&read) {
            continue;
        }
        hub.seen.insert(topic, read);
        hub.followers
            .retain(|follower| !follower.topics.contains(&topic) || (follower.sink)(topic));
    }
    forget_unfollowed(&mut Control::get_mut(cx).hub);
}

/// What `topic`'s reads answer now, hashed.
fn fingerprint(topic: Topic, cx: &mut App) -> u64 {
    let read = match topic {
        Topic::App => vec![exec::status_value(cx)],
        Topic::Workspace => vec![exec::projects_value(cx), exec::tabs_value(cx)],
        Topic::Settings => vec![exec::settings_value(cx)],
    };
    let mut hasher = DefaultHasher::new();
    for answer in read {
        match answer {
            Ok(value) => value.to_string().hash(&mut hasher),
            Err(error) => error.message.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Marks the topics the settings feed whenever a setting changes, and applies a change of
/// the control policy.
pub(crate) fn follow_settings(cx: &mut App) -> Subscription {
    settings::observe_app(cx, |cx| {
        mark(&[Topic::App, Topic::Settings], cx);
        super::policy_changed(cx);
    })
}
