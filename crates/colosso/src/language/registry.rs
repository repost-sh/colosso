//! The servers running, one per kind and checkout. A server starts with the first file of
//! its kind opened in its checkout, unless the settings turn it off there; stops when the
//! settings turn it off, when the user stops it, or when its checkout leaves the sidebar;
//! once it has had no file open for ten minutes too, if the settings ask; and no more than
//! the setting's number run at once, the least recently used one stopping for another, an
//! idle one before a busy one.

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
    /// The project each checkout belongs to, by its main checkout: where the settings turn a
    /// server off.
    projects: HashMap<PathBuf, PathBuf>,
    /// Counts up with each use, so the least recently used server has the lowest.
    uses: u64,
    _settings: Subscription,
}

/// A server, and what the registry keeps of it.
struct Slot {
    server: Entity<Server>,
    used: u64,
    /// Counting down to stopping it, while it has no file open.
    idle: Option<Task<()>>,
    /// Whether the settings turn it off, so it isn't started.
    off: bool,
    /// Whether the user stopped it, so it isn't started until they start it again.
    held: bool,
    _activity: Subscription,
}

impl Slot {
    /// Stops the server, saying `why`; it's let run on.
    fn stop(&self, why: &str, cx: &mut App) {
        self.server
            .update(cx, |server, cx| server.stop(why, cx))
            .detach();
    }
}

/// Runs servers from `launcher`: the real ones in the app, a fake in tests.
pub fn install(launcher: Launcher, cx: &mut App) {
    let registry = cx.new(|cx| Registry {
        launcher,
        servers: HashMap::new(),
        projects: HashMap::new(),
        uses: 0,
        _settings: settings::on_change(cx, Registry::settings_changed),
    });
    cx.set_global(Languages(registry));
}

/// Calls `act` on the app's registry, if it has one.
pub(super) fn with_registry(cx: &mut App, act: impl FnOnce(&mut Registry, &mut Context<Registry>)) {
    if let Some(Languages(registry)) = cx.try_global::<Languages>().cloned() {
        registry.update(cx, act);
    }
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

/// Says the checkout at `root` belongs to the project whose main checkout is `project`.
pub fn locate(root: &Path, project: &Path, cx: &mut App) {
    with_registry(cx, |registry, cx| {
        if registry.projects.get(root).map(PathBuf::as_path) != Some(project) {
            registry.projects.insert(root.to_owned(), project.to_owned());
            registry.settings_changed(cx);
        }
    });
}

/// Stops the servers of the checkout at `root`, which has left the sidebar.
pub fn forget(root: &Path, cx: &mut App) {
    with_registry(cx, |registry, cx| {
        registry.projects.remove(root);
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
    /// The servers of the checkout at `root`, by name.
    pub fn servers_at(&self, root: &Path) -> Vec<Entity<Server>> {
        let mut servers: Vec<(Kind, Entity<Server>)> = (self.servers.iter())
            .filter(|((_, at), _)| at == root)
            .map(|((kind, _), slot)| (*kind, slot.server.clone()))
            .collect();
        servers.sort_by_key(|(kind, _)| kind.name());
        servers.into_iter().map(|(_, server)| server).collect()
    }

    /// The server of `kind` in the checkout at `root`, if it has been made.
    pub fn server(&self, kind: Kind, root: &Path) -> Option<Entity<Server>> {
        (self.servers.get(&(kind, root.to_owned()))).map(|slot| slot.server.clone())
    }

    /// The project the checkout at `root` belongs to, by its main checkout.
    pub fn project_of(&self, root: &Path) -> Option<&Path> {
        self.projects.get(root).map(PathBuf::as_path)
    }

    /// Whether the settings turn the server of `kind` off in the checkout at `root`.
    pub fn is_off(&self, kind: Kind, root: &Path) -> bool {
        (self.servers.get(&(kind, root.to_owned()))).is_some_and(|slot| slot.off)
    }

    /// The server for `kind` in `checkout`, made unless it's there, with `document` open on
    /// it; started if it's stopped, unless it's off or the user stopped it.
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
        let off = !self.runs(&key, cx);
        if !self.servers.contains_key(&key) {
            cx.notify();
        }
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
                off,
                held: false,
                _activity,
            }
        });
        slot.used = self.uses;
        slot.idle = None;
        let (server, waits) = (slot.server.clone(), slot.off || slot.held);
        let project = checkout.read(cx).project().cloned();
        server.update(cx, |server, cx| {
            server.open(document, cx);
            if let Some(project) = &project {
                server.follow_writes(project, cx);
            }
        });
        // Not one that failed or crashed: that waits for the user to start it again.
        if !waits && *server.read(cx).state() == super::State::Stopped {
            self.start(&key, cx);
        }
        server
    }

    /// Starts the server of `kind` in the checkout at `root` again, whatever it's doing and
    /// however often it crashed, as the user asked; one the settings turn off stays off.
    pub fn restart(&mut self, kind: Kind, root: &Path, cx: &mut Context<Self>) {
        let key = (kind, root.to_owned());
        self.uses += 1;
        let Some(slot) = self.servers.get_mut(&key) else {
            return;
        };
        slot.used = self.uses;
        slot.held = false;
        if slot.off {
            return;
        }
        let server = slot.server.clone();
        self.make_room(&key, cx);
        let launcher = self.launcher.clone();
        server.update(cx, |server, cx| server.retry(launcher, cx));
    }

    /// Stops the server of `kind` in the checkout at `root`, as the user asked: its files stay
    /// open on it, and it starts again only when the user starts it.
    pub fn stop(&mut self, kind: Kind, root: &Path, cx: &mut Context<Self>) {
        if let Some(slot) = self.servers.get_mut(&(kind, root.to_owned())) {
            slot.held = true;
            slot.stop("you stopped it", cx);
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

    /// Whether the settings run the server of `key`, in its checkout's project.
    fn runs(&self, (kind, root): &Key, cx: &App) -> bool {
        settings::get(cx).runs_server(kind.name(), self.project_of(root))
    }

    /// Stops the servers the settings now turn off, and starts the ones they turn on again.
    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<Key> = self.servers.keys().cloned().collect();
        for key in keys {
            let runs = self.runs(&key, cx);
            let Some(slot) = self.servers.get_mut(&key) else {
                continue;
            };
            if slot.off != runs {
                continue;
            }
            slot.off = !runs;
            cx.notify();
            if runs {
                if !slot.held {
                    self.start(&key, cx);
                }
            } else {
                slot.stop("it's turned off in the settings", cx);
            }
        }
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
        let Some(most) = settings::get(cx).most_language_servers.map(|most| most.max(1)) else {
            return;
        };
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
                // Its files stay open; its menu in the status bar starts it again.
                slot.stop(&why, cx);
            }
        }
    }

    /// The server of `key` opened its first file, or closed its last: an idle one stops once
    /// it has stayed idle long enough, if the settings ask.
    fn activity(&mut self, key: &Key, activity: Activity, cx: &mut Context<Self>) {
        let stops_idle = settings::get(cx).stop_idle_language_servers;
        let Some(slot) = self.servers.get_mut(key) else {
            return;
        };
        slot.idle = match activity {
            Activity::Idle if stops_idle => {
                let key = key.clone();
                Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(IDLE).await;
                    let why = format!("no file was open for {} minutes", IDLE.as_secs() / 60);
                    this.update(cx, |this, cx| this.retire(&key, &why, cx)).ok();
                }))
            }
            Activity::Busy | Activity::Idle => None,
        };
    }

    /// Stops the server of `key` and lets go of it.
    fn retire(&mut self, key: &Key, why: &str, cx: &mut Context<Self>) {
        if let Some(slot) = self.servers.remove(key) {
            slot.stop(why, cx);
            cx.notify();
        }
    }
}
