//! What an agent asks of the desktop (`docs/agent-desktop.md` 10), answered at once or, for
//! what takes time, later by the loop.

use std::time::{Duration, Instant};

use koyo_desk_wire::Point;
use koyo_desk_wire::link::{Act, Answer, Ask, Close, Refusal, RefusalCode, RequestId, Window};

use crate::apps;
use crate::ax::Done;
use crate::host::{Host, Waiting, no_desktop};
use crate::lending::{Back, Wish};
use crate::snapshot::Snapshot;

/// A wait's quiet time and limit, as far as they go.
const QUIET: (u64, u64) = (100, 10_000);
const LIMIT: u64 = 120_000;
/// `cmd+w`.
const W: u16 = 13;

impl Host {
    /// Does `ask`; `None` when the answer comes later.
    pub fn ask(&mut self, request: RequestId, ask: Ask) -> Option<Result<Answer, Refusal>> {
        if self.desktop.is_none() {
            return Some(Err(no_desktop(&self.missing)));
        }
        match ask {
            Ask::Open {
                app,
                document,
                args,
                theirs,
                consent,
            } => {
                let wish = Wish { theirs, consent };
                self.open(request, &app, document.as_deref(), &args, wish)
            }
            Ask::Windows => {
                self.list();
                let windows = self.local(&self.windows);
                Some(Ok(Answer::Windows { windows }))
            }
            Ask::Screenshot { window, scale } => self.screenshot(request, window, scale),
            Ask::Snapshot {
                window,
                interactive,
                menus,
            } => Some(self.snapshot(window, interactive, menus)),
            Ask::Act(Act::Type { text, into }) => match self.begin_typing(request, &text, into) {
                Ok(()) => None,
                Err(refusal) => Some(Err(refusal)),
            },
            Ask::Act(act) => Some(self.act(act)),
            Ask::Wait {
                quiet_ms,
                timeout_ms,
            } => {
                let now = Instant::now();
                // The picture is only watched from now on: what came before doesn't count.
                self.changed = now;
                self.waits.push(Waiting {
                    request,
                    quiet: Duration::from_millis(quiet_ms.clamp(QUIET.0, QUIET.1)),
                    limit: Duration::from_millis(timeout_ms.min(LIMIT)),
                    started: now,
                });
                self.capture();
                None
            }
            Ask::Close(close) => Some(self.close(close).map(|()| Answer::Done)),
            Ask::TakeFocus { window, consent } => Some(self.take_focus(window, consent.as_deref())),
            Ask::ReturnFocus => Some(self.return_focus()),
        }
    }

    fn screenshot(
        &mut self,
        request: RequestId,
        window: Option<u32>,
        scale: u8,
    ) -> Option<Result<Answer, Refusal>> {
        if !self.grants.screen_recording {
            return Some(Err(Refusal::new(
                RefusalCode::NotPermitted,
                "Screen Recording isn't granted to the agent desktop's host: the person grants \
                 it in Settings → Agent desktop",
            )));
        }
        self.list();
        let Some(bounds) = self.desktop.as_ref().map(|desktop| desktop.display) else {
            return Some(Err(no_desktop(&self.missing)));
        };
        let (windows, area) = match window {
            Some(id) => match self.listed(id) {
                Ok(found) => (vec![found.clone()], found.frame),
                Err(gone) => return Some(Err(gone)),
            },
            None => (self.windows.clone(), bounds),
        };
        let origin = self.local_point(Point {
            x: area.x,
            y: area.y,
        });
        self.shots.ask(request, windows, (area, origin), scale);
        None
    }

    fn snapshot(
        &mut self,
        window: Option<u32>,
        interactive: bool,
        menus: bool,
    ) -> Result<Answer, Refusal> {
        self.list();
        if let Some(id) = window {
            self.listed(id)?;
        }
        let wanted: Vec<_> = (self.windows.iter())
            .filter(|listed| window.is_none_or(|id| id == listed.id))
            .cloned()
            .collect();
        let origin = self
            .desktop
            .as_ref()
            .map(|desktop| desktop.display)
            .unwrap_or_default();
        let mut snapshot = Snapshot::new(interactive, origin, self.next_ref);
        let mut with_menus = Vec::new();
        for listed in &wanted {
            let Some(app) = self.apps.iter_mut().find(|app| app.pid == listed.pid) else {
                continue;
            };
            if menus && !with_menus.contains(&app.pid) {
                with_menus.push(app.pid);
                snapshot.heading(&format!("- menubar of {}", app.name));
                if let Some(bar) = app.element().and_then(|app| app.element("AXMenuBar")) {
                    // But for the first: the Apple menu is the person's session's (Log Out,
                    // Shut Down, Sleep), not the app's.
                    for item in bar.children().into_iter().skip(1) {
                        snapshot.walk(&item, 1, 1);
                    }
                }
            }
            let frame = self
                .desktop
                .as_ref()
                .map_or(listed.frame, |d| d.local(listed.frame));
            snapshot.heading(&format!(
                "- window {} {:?} of {} at {:.0},{:.0} size {:.0}x{:.0}",
                listed.id, listed.title, listed.app, frame.x, frame.y, frame.width, frame.height
            ));
            match app.window(listed.id) {
                Some(element) => {
                    for child in element.children() {
                        snapshot.walk(&child, 1, 1);
                    }
                }
                None => snapshot.heading("  - (the app gives no tree for this window)"),
            }
        }
        let answer = Answer::Snapshot {
            elements: snapshot.elements(),
            truncated: snapshot.truncated,
            tree: snapshot.tree,
        };
        self.next_ref += snapshot.refs.len() as u64;
        self.refs = snapshot.refs;
        Ok(answer)
    }

    fn close(&mut self, close: Close) -> Result<(), Refusal> {
        match close {
            Close::Window(id) => {
                self.list();
                let listed = self.listed(id)?.clone();
                let element = (self.apps.iter_mut())
                    .find(|app| app.pid == listed.pid)
                    .and_then(|app| app.window(id));
                let pressed = element
                    .and_then(|element| element.element("AXCloseButton"))
                    .is_some_and(|button| button.act("AXPress") != Done::No);
                if !pressed {
                    // No close button to press: the app's own shortcut for it.
                    let target = self.target_of(&listed);
                    self.aim(target);
                    self.poster
                        .press(target, W, koyo_desk_wire::keys::flags::COMMAND);
                }
                Ok(())
            }
            Close::App(pid) => {
                let app = (self.apps.iter_mut())
                    .find(|app| app.pid == pid)
                    .ok_or_else(|| {
                        let message = format!("no app {pid} that the agent opened");
                        Refusal::new(RefusalCode::NotFound, message)
                    })?;
                // The person's own goes back to them as it is; the agent's is quit.
                if app.lent {
                    self.give_back(pid, Back::Quiet);
                } else {
                    apps::quit(&app.running, false);
                    app.quitting = Some(Instant::now());
                }
                Ok(())
            }
        }
    }
}

impl Host {
    /// The agent's window `id` as it's listed, or the refusal that says there's none.
    pub fn listed(&self, id: u32) -> Result<&Window, Refusal> {
        (self.windows.iter())
            .find(|listed| listed.id == id)
            .ok_or_else(|| no_window(id))
    }
}

pub fn no_window(id: u32) -> Refusal {
    let message = format!("no window {id} on the agent desktop: desktop_windows lists them");
    Refusal::new(RefusalCode::NotFound, message)
}
