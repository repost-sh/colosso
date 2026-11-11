//! What Settings → Devices does to Koyo's simulators, at the person's click: makes one, stops
//! one, deletes one. Each is `simctl` off the main thread, then the set listed again; what went
//! wrong is said on the page.

use gpui_kit::Context;

use super::{Devices, Link, set_dir, simulators};

impl Devices {
    /// Makes a simulator of `model` on the newest system, saying so on the page.
    pub fn make_new(&mut self, model: String, cx: &mut Context<Self>) {
        self.said = Some(format!("Making {model}…"));
        cx.notify();
        self.make(Some(model), cx, |made, cx| {
            super::devices(cx).update(cx, |devices, cx| {
                devices.said = Some(match made {
                    Ok(made) => format!("{} is made: a new Device tab opens it.", made.name),
                    Err(why) => why,
                });
                cx.notify();
            });
        });
    }

    /// Stops the simulator `udid`: through its host when Koyo runs it (the set is listed again
    /// once the host has shut it down), else as `simctl` does.
    pub fn stop(&mut self, udid: &str, cx: &mut Context<Self>) {
        if let Some(link) = self.existing(udid).filter(|link| link.read(cx).holds_simulator()) {
            link.update(cx, Link::stop);
            return;
        }
        self.after(udid, "stopped", simulators::shut_down, cx);
    }

    /// Deletes the simulator `udid`, and what it held on disk.
    pub fn delete(&mut self, udid: &str, cx: &mut Context<Self>) {
        self.after(udid, "deleted", simulators::delete, cx);
    }

    /// Does `what` to `udid` off the main thread, then lists the set again.
    fn after(&mut self, udid: &str, done: &'static str, what: fn(&std::path::Path, &str) -> Result<(), String>, cx: &mut Context<Self>) {
        let Some(set) = set_dir() else {
            return;
        };
        let name = self.simulator(udid).map_or_else(|| udid.to_owned(), |simulator| simulator.name.clone());
        let udid = udid.to_owned();
        let did = cx.background_executor().spawn(async move { what(&set, &udid) });
        cx.spawn(async move |this, cx| {
            let did = did.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(why) = did {
                    this.said = Some(format!("{name} wasn't {done}: {why}"));
                }
                this.refresh(cx);
            });
        })
        .detach();
    }
}
