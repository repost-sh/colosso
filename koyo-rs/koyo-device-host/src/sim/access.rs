//! A simulator's accessibility, on only while its tree is read (`docs/devices.md` 6).
//!
//! An app has a tree to read only if `ApplicationAccessibilityEnabled` is on in the simulator's
//! own settings, and every app started while it is on loads its accessibility bundles: Settings
//! 109 of them, 1.4 s of CPU more on a launch of 3.4 (measured, iOS 27.2). The simulator's own
//! accessibility service switches it on when it first starts after a boot, and nothing switches
//! it off again, across boots: a simulator read once paid for it ever after. So the host
//! switches it: on before a read (an app that runs loads its bundles when told, within the
//! second), off once no read has come for a minute, and off as the host goes. A simulator only
//! the person uses never has it on.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{ax, simctl};

const DOMAIN: &str = "com.apple.Accessibility";
/// What decides whether an app started from here on loads its bundles and has a tree.
const APPS: &str = "ApplicationAccessibilityEnabled";
/// What the simulator's service sets beside it; switched off with it.
const WHOLE: &str = "AccessibilityEnabled";
/// What tells the apps that run: without both, one that runs doesn't load its bundles
/// (measured).
const NOTICES: [&str; 2] = ["com.apple.accessibility.cache.app.ax", "com.apple.accessibility.cache.ax"];
/// How long each `simctl spawn` may take.
const EACH: Duration = Duration::from_secs(10);

/// What says, in milliseconds, how long after its tree was last read accessibility stays on,
/// for the suite's row: a minute without it.
pub const ENV_RESTS_MS: &str = "KOYO_DEVICE_AX_RESTS_MS";

/// What the jobs share of a simulator's accessibility.
#[derive(Default)]
pub struct Access {
    set: String,
    udid: String,
    /// The connection to its service, kept until it breaks.
    tree: Option<Arc<ax::Tree>>,
    /// Whether this host switched it on.
    on: bool,
    /// When it's switched off unless its tree is read before. From the start, for what an
    /// earlier host, killed, left on.
    rests: Option<Instant>,
    /// How long after a read that is.
    after: Duration,
}

/// Locked only to take the connection or to switch: never waited on by the host's loop.
pub type Shared = Arc<Mutex<Access>>;

impl Access {
    /// For the simulator `udid` of `set`.
    pub fn of(set: &str, udid: &str) -> Shared {
        let asked = std::env::var(ENV_RESTS_MS).ok().and_then(|ms| ms.parse().ok());
        let after = asked.map_or(Duration::from_secs(60), Duration::from_millis);
        Arc::new(Mutex::new(Access {
            set: set.to_owned(),
            udid: udid.to_owned(),
            rests: Some(Instant::now() + after),
            after,
            ..Access::default()
        }))
    }

    pub fn connected(&self) -> bool {
        self.tree.is_some()
    }

    fn spawn(&self, args: &[&str]) -> Result<String, String> {
        let mut all = vec!["spawn", self.udid.as_str()];
        all.extend(args);
        simctl::run(&self.set, &all, EACH)
    }

    /// Off in this host's books even if a step fails: the next read switches it on again,
    /// whatever it is.
    fn switch(&mut self, on: bool) -> Result<(), String> {
        self.run(&steps(on))?;
        self.on = on;
        Ok(())
    }

    fn run(&mut self, steps: &[Vec<&'static str>]) -> Result<(), String> {
        self.on = false;
        steps.iter().try_for_each(|step| self.spawn(step).map(|_| ()))
    }
}

/// What runs in the simulator to switch it, in order.
fn steps(on: bool) -> Vec<Vec<&'static str>> {
    let write = |key| vec!["defaults", "write", DOMAIN, key, "-bool", if on { "true" } else { "false" }];
    let mut steps = vec![write(APPS)];
    if !on {
        steps.push(write(WHOLE));
    }
    // Both in one call: each `simctl spawn` is a quarter of a second.
    steps.push(vec!["notifyutil", "-p", NOTICES[0], "-p", NOTICES[1]]);
    steps
}

/// Runs `read` over the kept connection, made from `port` when there is none, and let go when
/// it broke (interrupted, or the service gone), to be made again. Accessibility is switched on
/// first when it isn't.
pub fn with_tree<T>(shared: &Shared, port: Option<u32>, read: impl FnOnce(&ax::Tree) -> Result<T, String>) -> Result<T, String> {
    let tree = {
        let mut held = shared.lock().map_err(|error| error.to_string())?;
        if !held.on {
            held.switch(true)?;
        }
        held.rests = Some(Instant::now() + held.after);
        if let Some(tree) = held.tree.clone().filter(|tree| !tree.broken()) {
            tree
        } else {
            let port = port.ok_or("the simulator's accessibility isn't connected yet")?;
            let tree = Arc::new(ax::Tree::connect(port)?);
            held.tree = Some(Arc::clone(&tree));
            tree
        }
    };
    let read = read(&tree);
    if let Ok(mut held) = shared.lock() {
        held.rests = Some(Instant::now() + held.after);
        if tree.broken() {
            held.tree = None;
        }
    }
    read
}

/// Switches it on, if it isn't, ahead of what is about to need it: an app an agent starts has
/// its tree from its start.
pub fn wanted(shared: &Shared) {
    if let Ok(mut held) = shared.lock()
        && !held.on
        && held.switch(true).is_ok()
    {
        held.rests = Some(Instant::now() + held.after);
    }
}

/// Switches it off once its tree has gone unread long enough and nothing is `busy` reading it:
/// on a thread of its own, the lock held meanwhile, so a read that comes waits and switches it
/// on again.
pub fn rest(shared: &Shared, busy: bool) {
    let Ok(mut held) = shared.try_lock() else {
        return;
    };
    if busy || held.rests.is_none_or(|at| Instant::now() < at) {
        return;
    }
    held.rests = None;
    drop(held);
    let shared = Arc::clone(shared);
    std::thread::spawn(move || {
        let Ok(mut held) = shared.lock() else {
            return;
        };
        // A read came first: it's wanted on. Not switched on by this host, it's switched off
        // only if an earlier one left it on.
        let left_on = || held.spawn(&["defaults", "read", DOMAIN, APPS]).is_ok_and(|said| said.trim() == "1");
        if held.rests.is_some() || !(held.on || left_on()) {
            return;
        }
        if let Err(why) = held.switch(false) {
            crate::link::debug(&format!("the simulator's accessibility wasn't switched off: {why}"));
        }
    });
}

/// As the host goes: off, if this host switched it on.
pub fn leave(shared: &Shared) {
    if let Ok(mut held) = shared.lock()
        && held.on
    {
        // The settings only: nothing that runs is left to tell, the simulator shuts down next.
        let _ = held.run(&steps(false)[..2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switched_on_by_one_setting_and_both_notices() {
        let steps = steps(true);
        assert_eq!(steps[0], ["defaults", "write", DOMAIN, APPS, "-bool", "true"]);
        assert_eq!(steps[1], ["notifyutil", "-p", NOTICES[0], "-p", NOTICES[1]]);
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn switched_off_with_what_the_service_set_beside_it() {
        let steps = steps(false);
        assert_eq!(steps[0], ["defaults", "write", DOMAIN, APPS, "-bool", "false"]);
        assert_eq!(steps[1], ["defaults", "write", DOMAIN, WHOLE, "-bool", "false"]);
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn a_real_device_has_none_to_switch() {
        let shared = Shared::default();
        rest(&shared, false);
        leave(&shared);
        assert!(shared.lock().is_ok_and(|held| !held.on && held.rests.is_none()));
    }
}
