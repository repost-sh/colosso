//! Real iPhones and iPads by cable (`docs/devices.md` 5, 9): the ones `devicectl list devices`
//! lists, each with the first step it's missing to be driven, read off the main thread when a
//! Devices surface first shows and again whenever the watcher says one was plugged in or pulled
//! out (and once more a moment later: a device just plugged in takes that to be listed). The
//! watcher is the host's own program run as `--watch`, which listens to usbmuxd: idevice's
//! runtime stays out of the app, and only a device's setup, while it's open, looks on a timer.

use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use gpui_kit::Context;
use koyo_device_wire::{Missing, real};
use serde_json::Value;

use super::{Devices, Link, Standing};

/// How long after a plug the devices are listed once more.
const AGAIN_AFTER: Duration = Duration::from_secs(3);
/// How long after the watcher went it may be started again.
const WATCH_AGAIN_AFTER: Duration = Duration::from_secs(60);

/// A real device plugged in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connected {
    pub udid: String,
    /// The person's own name for it: `Sam's iPad`.
    pub name: String,
    pub ipad: bool,
    /// Its system: `iPadOS 27.0.1`.
    pub system: String,
    /// The first step it's missing to be driven, in the words the tab says.
    pub missing: Option<(Missing, String)>,
}

/// The iPhones and iPads `devicectl` lists by cable, its answer kept a moment in Koyo's devices
/// folder.
pub fn list() -> Result<Vec<Connected>, String> {
    if cfg!(test) {
        return Ok(Vec::new());
    }
    let scratch = super::state_dir().ok_or("Koyo has no folder to keep its devices in.")?;
    std::fs::create_dir_all(&scratch).map_err(|error| error.to_string())?;
    Ok(parse(&real::devicectl(&["list", "devices"], &scratch)?))
}

/// The devices of `devicectl list devices -j`'s answer that are plugged in.
pub fn parse(listed: &Value) -> Vec<Connected> {
    let mut connected: Vec<Connected> = real::devices(listed)
        .filter(|device| device["connectionProperties"]["transportType"] == "wired")
        .filter_map(|device| {
            let kind = device["hardwareProperties"]["deviceType"].as_str()?;
            if kind != "iPhone" && kind != "iPad" {
                return None;
            }
            let name = device["deviceProperties"]["name"].as_str().unwrap_or(kind).to_owned();
            Some(Connected {
                udid: device["hardwareProperties"]["udid"].as_str()?.to_owned(),
                missing: real::first_missing(Some(device), &name),
                ipad: kind == "iPad",
                system: real::system(device),
                name,
            })
        })
        .collect();
    connected.sort_by(|a, b| a.name.cmp(&b.name).then(a.udid.cmp(&b.udid)));
    connected
}

/// The watcher, a child of Koyo's: it ends when its standard input closes, as it does when this
/// is dropped or Koyo goes.
pub struct Watcher {
    _child: Child,
}

/// Starts the watcher; `heard` is called on a thread of its own each time a device is plugged
/// in or pulled out, and the thread ends when the watcher does.
pub fn watch(program: &Path, heard: impl Fn() + Send + 'static) -> Result<Watcher, String> {
    let mut child = Command::new(program)
        .arg("--watch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("the device watcher didn't start: {error}"))?;
    let out = child.stdout.take().ok_or("the device watcher has no output")?;
    std::thread::Builder::new()
        .name("device-watcher".into())
        .spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if line.starts_with("attached") || line == "detached" {
                    heard();
                }
            }
        })
        .map_err(|error| format!("no thread for the device watcher: {error}"))?;
    Ok(Watcher { _child: child })
}

impl Devices {
    /// What's plugged in, as last listed: nothing while that isn't known.
    pub fn plugged(&self) -> &[Connected] {
        match &self.connected {
            Some(Ok(listed)) => listed,
            _ => &[],
        }
    }

    /// The device `udid` plugged in, as last listed.
    pub fn plugged_device(&self, udid: &str) -> Option<&Connected> {
        self.plugged().iter().find(|device| device.udid == udid)
    }

    /// Lists what's plugged in again, unless that is under way; and watches for plugs, unless
    /// the watcher runs or went a moment ago.
    pub fn look_connected(&mut self, cx: &mut Context<Self>) {
        if self.listing_connected || cfg!(test) {
            return;
        }
        let watch_again = self.watcher_went.is_none_or(|went| went.elapsed() >= WATCH_AGAIN_AFTER);
        if self.watcher.is_none() && watch_again {
            self.watch_plugs(cx);
        }
        self.listing_connected = true;
        let since = Instant::now();
        let listed = cx.background_executor().spawn(async move { list() });
        cx.spawn(async move |this, cx| {
            let listed = listed.await;
            let _ = this.update(cx, |this, cx| {
                this.listing_connected = false;
                this.connected_afresh(listed, since, cx);
            });
        })
        .detach();
    }

    /// What's plugged in, listed afresh from `since` (by a look of the registry's, or
    /// `device_list`).
    pub fn connected_afresh(&mut self, listed: Result<Vec<Connected>, String>, since: Instant, cx: &mut Context<Self>) {
        let changed = self.connected.as_ref() != Some(&listed);
        // Unplugged, a device's lend ends, whether or not a tab showed it.
        if let Ok(now) = &listed {
            let gone: Vec<String> = (self.plugged().iter())
                .filter(|before| now.iter().all(|device| device.udid != before.udid))
                .map(|before| before.udid.clone())
                .collect();
            for udid in gone {
                self.lend_ends(&udid, cx);
            }
        }
        self.connected = Some(listed);
        self.back(since, cx);
        if changed {
            cx.notify();
        }
    }

    /// For the off-screen pictures: what's plugged in, as given; `devicectl` is never asked.
    #[cfg(feature = "visual-check")]
    pub fn plugged_in(&mut self, listed: Vec<Connected>, cx: &mut Context<Self>) {
        self.listing_connected = true;
        self.connected_afresh(Ok(listed), Instant::now(), cx);
    }

    /// For the off-screen walk with a real device: what the pictures set is let go, and what's
    /// plugged in is listed and watched as the app does.
    #[cfg(feature = "visual-check")]
    pub fn unpinned(&mut self, cx: &mut Context<Self>) {
        (self.connected, self.listing_connected) = (None, false);
        self.look_connected(cx);
    }

    /// Whether the watcher runs, keeping what's listed as plugged in fresh.
    pub fn watching(&self) -> bool {
        self.watcher.is_some()
    }

    /// Starts the watcher, and hears it on the main thread: a burst of plugs (each device there
    /// is said at its start) is one listing now and one a moment later. Once it goes (usbmuxd
    /// gone, as after a sleep), a later look starts it again.
    fn watch_plugs(&mut self, cx: &mut Context<Self>) {
        let Ok(program) = super::host::program() else {
            return;
        };
        let (told, mut heard) = futures::channel::mpsc::unbounded();
        match watch(&program, move || {
            let _ = told.unbounded_send(());
        }) {
            Ok(watcher) => self.watcher = Some(watcher),
            Err(why) => return log::warn!("{why}"),
        }
        cx.spawn(async move |this, cx| {
            while heard.next().await.is_some() {
                for wait in [Duration::ZERO, AGAIN_AFTER] {
                    cx.background_executor().timer(wait).await;
                    while heard.try_recv().is_ok() {}
                    if this.update(cx, Self::look_connected).is_err() {
                        return;
                    }
                }
            }
            let _ = this.update(cx, |this, _| {
                log::warn!("the device watcher went");
                (this.watcher, this.watcher_went) = (None, Some(Instant::now()));
            });
        })
        .detach();
    }

    /// A device whose tab ended for want of a step the list sees (plugged in, trusted,
    /// Developer Mode, its system, its services) starts again by itself once a listing begun
    /// after it ended says the device is ready, as one unplugged does when it's back. The link
    /// counts these starts: what the list can't see (another app holding the device) or a host
    /// that disagrees with the list never starts it in a loop.
    fn back(&mut self, since: Instant, cx: &mut Context<Self>) {
        let seen = |standing: Standing<'_>| {
            matches!(standing, Standing::Ended { missing, .. } if missing.is_none_or(|code| real::STEPS.contains(&code)))
        };
        let ready = self.plugged().iter().filter(|device| device.missing.is_none());
        let links: Vec<_> = ready.filter_map(|device| self.existing(&device.udid)).collect();
        for link in links {
            let due = {
                let link = link.read(cx);
                link.phone && link.shown_somewhere() && seen(link.standing()) && link.ended_at().is_some_and(|at| at < since)
            };
            if due {
                link.update(cx, Link::start_again);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn plugged_in_iphones_and_ipads_are_listed_with_what_they_miss() {
        let listed = json!({"result": {"devices": [
            {"hardwareProperties": {"udid": "IPAD", "deviceType": "iPad"},
             "connectionProperties": {"transportType": "wired", "pairingState": "paired"},
             "deviceProperties": {"name": "Sam's iPad", "developerModeStatus": "enabled", "osVersionNumber": "27.0.1", "ddiServicesAvailable": true}},
            {"hardwareProperties": {"udid": "OLD", "deviceType": "iPhone"},
             "connectionProperties": {"transportType": "wired", "pairingState": "paired"},
             "deviceProperties": {"name": "An iPhone", "developerModeStatus": "enabled", "osVersionNumber": "26.6.1", "ddiServicesAvailable": false}},
            {"hardwareProperties": {"udid": "AIR", "deviceType": "iPhone"},
             "connectionProperties": {"transportType": "localNetwork", "pairingState": "paired"},
             "deviceProperties": {"name": "Over the air", "osVersionNumber": "27.0"}},
            {"hardwareProperties": {"udid": "WATCH", "deviceType": "appleWatch"},
             "connectionProperties": {"transportType": "wired"}, "deviceProperties": {}}
        ]}});
        let connected = parse(&listed);
        let udids: Vec<&str> = connected.iter().map(|device| device.udid.as_str()).collect();
        assert_eq!(udids, ["OLD", "IPAD"], "by name; by cable only; iPhones and iPads only");
        assert_eq!(connected[1].missing, None);
        assert!(connected[1].ipad);
        assert_eq!(connected[1].system, "iPadOS 27.0.1");
        let (code, why) = connected[0].missing.clone().unwrap_or((Missing::Other, String::new()));
        assert_eq!(code, Missing::System);
        assert!(why.contains("An iPhone runs iOS 26.6.1"), "{why}");
    }
}
