//! Devices in Koyo (`docs/devices.md`): the app's side. One [`Devices`] keeps Koyo's own
//! simulators (read from its device set when a Devices surface wants them), a link per open
//! device ([`Link`]), and starts each device's host when its tab asks. Koyo's quit tells every
//! host to go and doesn't wait: each shuts its simulator down by itself, and what a killed host
//! left booted is shut down at the next start, from its record.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::{App, AppContext as _, Context, Entity, Global};

mod canvas;
mod control;
mod host;
mod link;
mod picture;
mod screens;
pub(crate) mod simulators;
mod touch;
mod typing;
mod view;

pub use link::{Link, State};
pub use simulators::Simulator;
pub use view::{Chosen, DeviceEvent, DeviceView, bind_keys};

use crate::settings;

pub struct Devices {
    /// Koyo's own simulators, as its set last listed them; `None` until asked.
    pub simulators: Option<Result<Vec<Simulator>, String>>,
    listing: bool,
    links: HashMap<String, Entity<Link>>,
}

struct TheDevices(Entity<Devices>);

impl Global for TheDevices {}

/// Where Koyo keeps its devices: its own device set, and its hosts' locks, records and logs.
pub fn folder() -> Option<PathBuf> {
    settings::data_dir().map(|data| data.join("devices"))
}

/// Koyo's own device set; in a debug build, the one `KOYO_DEVICE_SET` names (QA keeps its
/// simulators in the checkout).
pub(crate) fn set_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(named) = std::env::var_os("KOYO_DEVICE_SET")
    {
        return Some(named.into());
    }
    folder().map(|folder| folder.join("set"))
}

fn state_dir() -> Option<PathBuf> {
    folder().map(|folder| folder.join("state"))
}

/// What a host of `udid` is started with.
fn launch(udid: &str) -> Result<host::Launch, String> {
    let (Some(set), Some(state)) = (set_dir(), state_dir()) else {
        return Err("Koyo has no folder to keep its devices in.".into());
    };
    Ok(host::Launch {
        program: host::program()?,
        udid: udid.to_owned(),
        set,
        state,
    })
}

/// Makes the registry, and shuts down what a host killed last time left booted.
pub fn init(cx: &mut App) {
    let devices = cx.new(|_| Devices {
        simulators: None,
        listing: false,
        links: HashMap::new(),
    });
    cx.set_global(TheDevices(devices.clone()));
    if !cfg!(test)
        && let (Some(set), Some(state)) = (set_dir(), state_dir())
    {
        cx.background_executor()
            .spawn(async move { simulators::shut_down_leftovers(&set, &state) })
            .detach();
    }
    cx.on_app_quit(move |cx| {
        devices.update(cx, Devices::stop_all);
        async {}
    })
    .detach();
}

pub fn devices(cx: &App) -> Entity<Devices> {
    cx.global::<TheDevices>().0.clone()
}

impl Devices {
    /// The link of `udid`, made the first time it's asked for.
    pub fn link(&mut self, udid: &str, name: &str, cx: &mut Context<Self>) -> Entity<Link> {
        self.links
            .entry(udid.to_owned())
            .or_insert_with(|| cx.new(|cx| Link::new(udid.to_owned(), name.to_owned(), cx)))
            .clone()
    }

    pub fn existing(&self, udid: &str) -> Option<Entity<Link>> {
        self.links.get(udid).cloned()
    }

    /// Every link, for the tools and Settings.
    pub fn links(&self) -> impl Iterator<Item = &Entity<Link>> {
        self.links.values()
    }

    /// Lists Koyo's simulators again, unless that is under way: on a Devices surface's first
    /// showing, and after Koyo made or deleted one. Never on a timer.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.listing || cfg!(test) {
            return;
        }
        let Some(set) = set_dir() else {
            self.simulators = Some(Err("Koyo has no folder to keep its devices in.".into()));
            cx.notify();
            return;
        };
        self.listing = true;
        let listed = cx
            .background_executor()
            .spawn(async move { simulators::list(&set) });
        cx.spawn(async move |this, cx| {
            let listed = listed.await;
            let _ = this.update(cx, |this, cx| {
                this.listing = false;
                this.simulators = Some(listed);
                cx.notify();
            });
        })
        .detach();
    }

    /// Makes a simulator of Koyo's own, the newest iPhone on the newest system installed, and
    /// lists again; `done` gets its UDID.
    pub fn make_default(&mut self, cx: &mut Context<Self>, done: impl FnOnce(Result<Simulator, String>, &mut App) + 'static) {
        let Some(set) = set_dir() else {
            return;
        };
        let made = cx
            .background_executor()
            .spawn(async move { simulators::make_default(&set) });
        cx.spawn(async move |this, cx| {
            let made = made.await;
            let _ = this.update(cx, Devices::refresh);
            cx.update(|cx| done(made, cx));
        })
        .detach();
    }

    /// Koyo quits: every host is told to go.
    fn stop_all(&mut self, cx: &mut Context<Self>) {
        for link in self.links.values() {
            link.update(cx, Link::stop);
        }
    }

    /// What the set listed just now, read elsewhere (the device tools list it afresh).
    pub(crate) fn listed_afresh(&mut self, simulators: Vec<Simulator>, cx: &mut Context<Self>) {
        self.simulators = Some(Ok(simulators));
        cx.notify();
    }

    /// For the off-screen pictures: what the set would have listed.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn listed(&mut self, simulators: Result<Vec<Simulator>, String>, cx: &mut Context<Self>) {
        self.simulators = Some(simulators);
        self.listing = true;
        cx.notify();
    }

    /// The simulator of Koyo's own whose UDID is `udid`, as last listed.
    pub fn simulator(&self, udid: &str) -> Option<&Simulator> {
        match &self.simulators {
            Some(Ok(all)) => all.iter().find(|s| s.udid == udid),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;
