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
pub(crate) mod needs;
mod picture;
mod screens;
pub mod setup;
pub(crate) mod simulators;
mod standing;
mod touch;
mod typing;
mod view;

pub use link::Link;
pub use needs::{Found, Need};
#[cfg(test)]
pub use link::State;
pub use simulators::Simulator;
pub use standing::Standing;
pub use view::{Chosen, DeviceEvent, DeviceView, bind_keys};

use crate::settings;

pub struct Devices {
    /// Koyo's own simulators, as its set last listed them; `None` until asked.
    pub simulators: Option<Result<Vec<Simulator>, String>>,
    /// What the Mac has for them, read with the list; `None` until then.
    pub found: Option<Found>,
    listing: bool,
    /// Whether the setup came up by itself since Koyo started: it does once.
    offered: bool,
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
        found: None,
        listing: false,
        offered: false,
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
    /// showing, and after Koyo made or deleted one. Never on a timer. What the Mac has is read
    /// with it while that isn't known, or something was missing.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let whole = self.found.as_ref().is_none_or(|found| found.need().is_some());
        self.look(whole, false, cx);
    }

    /// Reads what the Mac has afresh, with the list: the setup opening, and `Check Again`.
    pub fn look_again(&mut self, cx: &mut Context<Self>) {
        self.look(true, false, cx);
    }

    /// The setup's look while it waits on the person: the checks found passing are spared.
    pub fn look_along(&mut self, cx: &mut Context<Self>) {
        self.look(true, true, cx);
    }

    fn look(&mut self, whole: bool, spare: bool, cx: &mut Context<Self>) {
        if self.listing || cfg!(test) {
            return;
        }
        let Some(set) = set_dir() else {
            self.simulators = Some(Err("Koyo has no folder to keep its devices in.".into()));
            cx.notify();
            return;
        };
        self.listing = true;
        let known = self.found.clone().filter(|_| spare);
        let looked = cx.background_executor().spawn(async move {
            if whole {
                let (found, listed) = needs::find(&set, known.as_ref());
                (Some(found), listed)
            } else {
                (None, Some(simulators::list(&set)))
            }
        });
        cx.spawn(async move |this, cx| {
            let (found, listed) = looked.await;
            let _ = this.update(cx, |this, cx| {
                this.listing = false;
                if found.is_some() {
                    this.found = found;
                }
                if listed.is_some() {
                    this.simulators = listed;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The first thing Koyo's simulators miss, once it's known.
    pub fn need(&self) -> Option<Need> {
        let found = self.found.as_ref()?;
        found.need().or_else(|| matches!(&self.simulators, Some(Ok(all)) if all.is_empty()).then_some(Need::Device))
    }

    /// Whether the setup should come up by itself: something is missing, and it wasn't
    /// offered yet since Koyo started.
    pub fn setup_due(&self) -> bool {
        !self.offered && self.need().is_some()
    }

    /// [`Devices::setup_due`], which from now on it isn't: it's offered once.
    pub fn offers_setup(&mut self) -> bool {
        let due = self.setup_due();
        self.offered |= due;
        due
    }

    /// Makes a simulator of Koyo's own, of `model` (else the newest iPhone) on the newest
    /// system installed, and lists again; `done` gets it.
    pub fn make(&mut self, model: Option<String>, cx: &mut Context<Self>, done: impl FnOnce(Result<Simulator, String>, &mut App) + 'static) {
        let Some(set) = set_dir() else {
            cx.defer(|cx| done(Err("Koyo has no folder to keep its devices in.".into()), cx));
            return;
        };
        let made = cx
            .background_executor()
            .spawn(async move { simulators::make(&set, model.as_deref()).map_err(|unmade| unmade.to_string()) });
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

    /// For the off-screen pictures: what the set would have listed, and the Mac had.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn listed(&mut self, simulators: Result<Vec<Simulator>, String>, found: Option<Found>, cx: &mut Context<Self>) {
        self.simulators = Some(simulators);
        self.found = found;
        self.listing = true;
        cx.notify();
    }

    /// For the off-screen walk: what the pictures set is let go, so the next look is the Mac's.
    #[cfg(feature = "visual-check")]
    pub fn unlisted(&mut self, cx: &mut Context<Self>) {
        (self.simulators, self.found, self.listing) = (None, None, false);
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
