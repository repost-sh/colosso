//! Which window the keys go to (`docs/agent-desktop.md` 7): the one an act names, or the one
//! last clicked, and from there what its app has made of it since. An app moves its keys by
//! itself, as it would with the person at it: to the document it just made, to the panel it
//! opened, to the sheet that came down on its window.

use std::time::Duration;

use koyo_desk_wire::link::{Refusal, RefusalCode, Window};

use crate::host::Host;
use crate::input::Target;
use crate::{ns, skylight, windows};

impl Host {
    /// Finds the window keys go to and readies it for them ([`Host::aim`]); the keys that
    /// name no window go there too from now on. An app can be left with its keys in no
    /// window at all, as it is once a panel of its has closed, and then hears no key: it's
    /// asked to make the window its main one again.
    pub fn keys_to(&mut self, window: Option<u32>) -> Result<Target, Refusal> {
        let target = self.key_target(window)?;
        self.aim(target);
        self.focus = Some(target.window);
        let app = self.apps.iter_mut().find(|app| app.pid == target.pid);
        if let Some(app) = app
            && app.key_window().is_none()
            && let Some(element) = app.window(target.window)
        {
            element.set("AXMain", &ns::yes());
            ns::pause(Duration::from_millis(30));
        }
        Ok(target)
    }

    /// The window keys go to: `window` when it's named, else where they are now
    /// ([`Host::key_window`]).
    fn key_target(&mut self, window: Option<u32>) -> Result<Target, Refusal> {
        let wanted = match window {
            Some(id) => Some(windows::listed_for(&self.hosted, id)),
            None => self.key_window(),
        };
        let found = wanted.and_then(|id| self.windows.iter().find(|listed| listed.id == id));
        match (found, window) {
            (Some(listed), _) => Ok(self.target_of(listed)),
            (None, Some(id)) => Err(crate::asks::no_window(id)),
            (None, None) => Err(Refusal::new(
                RefusalCode::NotFound,
                "no window on the agent desktop to send keys to",
            )),
        }
    }

    /// Where the keys are when no window is named: in the window last clicked, or the
    /// frontmost; in the one its app made key since, if the app was told that window was
    /// key and has moved on by itself; and in the sheet on that window, if there is one.
    fn key_window(&mut self) -> Option<u32> {
        let listed =
            |host: &Host, id: u32| host.windows.iter().find(|window| window.id == id).cloned();
        let last = (self.focus)
            .and_then(|id| listed(self, id))
            .or_else(|| self.windows.first().cloned())?;
        let mut window = last.id;
        if self.keyed == Some(last.id) {
            let own = (self.apps.iter())
                .find(|app| app.pid == last.pid)
                .and_then(crate::app::App::key_window)
                .map(|id| windows::listed_for(&self.hosted, id))
                .and_then(|id| listed(self, id))
                .filter(|own| own.pid == last.pid);
            if let Some(own) = own {
                window = own.id;
                self.focus = Some(own.id);
            }
        }
        Some(sheet_on(&self.windows, window).unwrap_or(window))
    }
}

/// The sheet on the window `id`: the window in front of it that the window server keeps
/// together with it.
fn sheet_on(listed: &[Window], id: u32) -> Option<u32> {
    let together = skylight::together_with(id);
    (listed.iter())
        .take_while(|window| window.id != id)
        .find(|window| together.contains(&window.id))
        .map(|window| window.id)
}
