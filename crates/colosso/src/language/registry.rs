//! The servers running, one per kind and checkout. A server starts with the first file of
//! its kind opened in its checkout; stops once it has had none open for ten minutes, or when
//! its checkout leaves the sidebar; and no more than the setting's number run at once, the
//! least recently used one stopping for another, an idle one before a busy one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::{App, AppContext as _, Context, Entity, Global, Subscription, Task};

use super::providers::Attached;
use super::server::{Activity, Server};
use super::{IDLE, Kind, Launcher};
use crate::checkout::Checkout;
use crate::files::FileEditor;
use crate::settings;

/// The app's registry of servers.
#[derive(Clone)]
pub struct Languages(pub Entity<Registry>);

impl Global for Languages {}

type Key = (Kind, PathBuf);

pub struct Registry {
    launcher: Launcher,
    servers: HashMap<Key, Slot>,
    /// Counts up with each use, so the least recently used server has the lowest.
    uses: u64,
}

/// A server, and what the registry keeps of it.
struct Slot {
    server: Entity<Server>,
    used: u64,
    /// Counting down to stopping it, while it has no file open.
    idle: Option<Task<()>>,
    _activity: Subscription,
}

/// Runs servers from `launcher`: the real ones in the app, a fake in tests.
pub fn install(launcher: Launcher, cx: &mut App) {
    let registry = cx.new(|_| Registry {
        launcher,
        servers: HashMap::new(),
        uses: 0,
    });
    cx.set_global(Languages(registry));
}

/// Gives the file `view` shows to its kind's server for `checkout`, starting the server if
/// it isn't running. A file no server knows is left alone, as is every file without a
/// registry.
pub fn attach(checkout: &Entity<Checkout>, view: &Entity<FileEditor>, cx: &mut App) {
    let Some(Languages(registry)) = cx.try_global::<Languages>().cloned() else {
        return;
    };
    let document = view.read(cx).document().clone();
    let path = document.read(cx).path().clone();
    let Some(kind) = Kind::of(&path) else {
        return;
    };
    let server = registry.update(cx, |registry, cx| {
        registry.open(kind, checkout, &document, cx)
    });
    view.update(cx, |view, cx| {
        let attached = Attached::new(server, checkout.downgrade(), path, cx);
        view.attach(attached, cx);
    });
}

/// Stops the servers of the checkout at `root`, which has left the sidebar.
pub fn forget(root: &Path, cx: &mut App) {
    let Some(Languages(registry)) = cx.try_global::<Languages>().cloned() else {
        return;
    };
    registry.update(cx, |registry, cx| {
        let gone: Vec<Key> = (registry.servers.keys())
            .filter(|(_, at)| at == root)
            .cloned()
            .collect();
        for key in gone {
            registry.retire(&key, "its worktree left the sidebar", cx);
        }
    });
}

impl Registry {
    /// The server for `kind` in `checkout`, made unless it's there, with `document` open on
    /// it; started if it's stopped.
    fn open(
        &mut self,
        kind: Kind,
        checkout: &Entity<Checkout>,
        document: &Entity<crate::checkout::Document>,
        cx: &mut Context<Self>,
    ) -> Entity<Server> {
        let root = checkout.read(cx).root().clone();
        let key = (kind, root.clone());
        self.uses += 1;
        let slot = self.servers.entry(key.clone()).or_insert_with(|| {
            let server = cx.new(|_| Server::new(kind, root));
            let watched = key.clone();
            let _activity = cx.subscribe(&server, move |this, _, activity, cx| {
                this.activity(&watched, *activity, cx);
            });
            Slot {
                server,
                used: 0,
                idle: None,
                _activity,
            }
        });
        slot.used = self.uses;
        slot.idle = None;
        let server = slot.server.clone();
        let project = checkout.read(cx).project().cloned();
        server.update(cx, |server, cx| {
            server.open(document, cx);
            if let Some(project) = &project {
                server.follow_writes(project, cx);
            }
        });
        // Not one that failed or crashed: that waits for the user to retry it.
        if *server.read(cx).state() == super::State::Stopped {
            self.start(&key, cx);
        }
        server
    }

    /// Starts the server again after it failed or crashed, as the user asked.
    pub fn retry(&mut self, server: &Entity<Server>, cx: &mut Context<Self>) {
        let key = (server.read(cx).kind(), server.read(cx).root().to_owned());
        self.uses += 1;
        if let Some(slot) = self.servers.get_mut(&key) {
            slot.used = self.uses;
            self.make_room(&key, cx);
            let launcher = self.launcher.clone();
            server.update(cx, |server, cx| server.retry(launcher, cx));
        }
    }

    /// The servers, for tests and the visual check.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn servers(&self) -> Vec<Entity<Server>> {
        self.servers
            .values()
            .map(|slot| slot.server.clone())
            .collect()
    }

    /// Stops every server; the task ends once each has exited.
    #[cfg(feature = "visual-check")]
    pub fn stop_all(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let stopping: Vec<Task<()>> = (self.servers.drain())
            .map(|(_, slot)| slot.server.update(cx, |server, cx| server.stop("done", cx)))
            .collect();
        cx.background_spawn(async move {
            futures::future::join_all(stopping).await;
        })
    }

    fn start(&mut self, key: &Key, cx: &mut Context<Self>) {
        self.make_room(key, cx);
        let Some(slot) = self.servers.get(key) else {
            return;
        };
        let launcher = self.launcher.clone();
        slot.server
            .update(cx, |server, cx| server.start(launcher, cx));
    }

    /// Stops servers other than `key`'s until starting it keeps within the limit: the least
    /// recently used idle one first, and the least recently used of all when none is idle.
    fn make_room(&mut self, key: &Key, cx: &mut Context<Self>) {
        let most = settings::get(cx).most_language_servers.max(1);
        loop {
            let live: Vec<(&Key, &Slot)> = (self.servers.iter())
                .filter(|(other, slot)| *other != key && slot.server.read(cx).is_live())
                .collect();
            if live.len() < most {
                return;
            }
            let victim = live
                .into_iter()
                .min_by_key(|(_, slot)| (!slot.server.read(cx).is_idle(), slot.used))
                .map(|(victim, _)| victim.clone());
            let Some(victim) = victim else {
                return;
            };
            let why = format!("no more than {most} language servers run at once");
            if self
                .servers
                .get(&victim)
                .is_some_and(|slot| slot.server.read(cx).is_idle())
            {
                self.retire(&victim, &why, cx);
            } else if let Some(slot) = self.servers.get(&victim) {
                // Its files stay open; its status offers to start it again.
                slot.server
                    .update(cx, |server, cx| server.stop(&why, cx))
                    .detach();
            }
        }
    }

    /// The server of `key` opened its first file, or closed its last: an idle one stops once
    /// it has stayed idle long enough.
    fn activity(&mut self, key: &Key, activity: Activity, cx: &mut Context<Self>) {
        let Some(slot) = self.servers.get_mut(key) else {
            return;
        };
        slot.idle = match activity {
            Activity::Busy => None,
            Activity::Idle => {
                let key = key.clone();
                Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(IDLE).await;
                    let why = format!("no file was open for {} minutes", IDLE.as_secs() / 60);
                    this.update(cx, |this, cx| this.retire(&key, &why, cx)).ok();
                }))
            }
        };
    }

    /// Stops the server of `key` and lets go of it.
    fn retire(&mut self, key: &Key, why: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.servers.remove(key) {
            slot.server
                .update(cx, |server, cx| server.stop(why, cx))
                .detach();
        }
    }
}
