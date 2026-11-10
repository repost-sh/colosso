//! Getting the host its two grants (`docs/agent-desktop.md` 11). The person is taken through
//! them one at a time (`setup`): a process of the loader started for that alone has macOS
//! list it for one, Koyo opens its pane of System Settings, and the desk sees it given
//! without being told. Accessibility the host reads itself, a few times a second while it
//! waits. Screen Recording a running process never learns of, so a process of the loader
//! started afresh is asked every second, and once it says yes the host is started again by
//! itself, which is how that grant takes hold.

use std::time::{Duration, Instant};

use gpui_kit::Context;
use koyo_desk_wire::link::DesktopState;

use super::{Desk, State, host};
use crate::settings;

const ACCESSIBILITY: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
const SCREEN_RECORDING: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";

/// How long after the person was sent to System Settings the desk goes on looking for
/// what they give there.
const EXPECTED_FOR: Duration = Duration::from_secs(300);
/// How often a fresh process is asked whether Screen Recording was given.
const PROBED_EVERY: Duration = Duration::from_secs(1);

/// A privacy grant the host works by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    /// To make the desktop and act in its apps.
    Accessibility,
    /// To show the desktop and photograph it.
    ScreenRecording,
}

impl Grant {
    /// The grant as the loader is told it (`--ask`).
    fn said(self) -> &'static str {
        match self {
            Grant::Accessibility => "accessibility",
            Grant::ScreenRecording => "screen-recording",
        }
    }
}

/// Where the person is in giving the host what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The host is starting, or being started again: nothing to do but wait.
    Starting,
    /// A grant is missing, and is the next to give.
    Needs(Grant),
    /// Both are given and the desktop is made.
    Ready,
    /// Off, or it failed: not a matter of grants.
    Unavailable,
}

impl Desk {
    /// Where the setup stands.
    pub fn step(&self) -> Step {
        match (&self.state, self.grants, &self.desktop) {
            (State::Off | State::Failed(_), _, _) => Step::Unavailable,
            (State::Running, Some(grants), _) if !grants.accessibility => {
                Step::Needs(Grant::Accessibility)
            }
            (State::Running, Some(grants), _) if !grants.screen_recording => {
                Step::Needs(Grant::ScreenRecording)
            }
            (State::Running, Some(_), Some(DesktopState::Ready { .. })) => Step::Ready,
            // Starting, or running with its grants or its desktop not said yet.
            (State::Starting | State::Running, _, _) => Step::Starting,
        }
    }

    /// Whether the setup should come up by itself: a grant is missing, and it wasn't
    /// offered yet since Koyo started.
    pub fn setup_due(&self) -> bool {
        matches!(self.step(), Step::Needs(_)) && !self.offered
    }

    /// [`Desk::setup_due`], which from now on it isn't: it's offered once.
    pub fn offers_setup(&mut self) -> bool {
        let due = self.setup_due();
        self.offered |= due;
        due
    }

    /// Has macOS list the host for `grant`, by a process of the loader started for that
    /// alone, and opens the pane of System Settings it's given in. Every asking is a process
    /// of its own: one that macOS dropped is made good by asking again. From now on the desk
    /// looks for the grant.
    pub fn ask_grant(&mut self, grant: Grant, cx: &mut Context<Self>) {
        if let (Ok(loader), Some(dir)) = (host::loader(), desk_dir()) {
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = host::ask(&loader, &dir, grant.said()) {
                        log::warn!("the agent desktop's host couldn't ask for a grant: {error}");
                    }
                })
                .detach();
        }
        cx.open_url(match grant {
            Grant::Accessibility => ACCESSIBILITY,
            Grant::ScreenRecording => SCREEN_RECORDING,
        });
        self.expect_grants(cx);
    }

    /// Shows the host's app in Finder, for the person to drag into a list of System Settings
    /// that macOS didn't put it in.
    pub fn reveal_host(&self, cx: &mut Context<Self>) {
        if let (Ok(loader), Some(dir)) = (host::loader(), desk_dir()) {
            cx.reveal_path(&host::shown(&loader, &dir));
        }
    }

    /// The person is at giving the grants (asked for one, or the setup is open): the desk
    /// looks for them for a while from now.
    pub fn expect_grants(&mut self, cx: &mut Context<Self>) {
        self.expected = Some(Instant::now());
        self.probe_along(cx);
    }

    /// The setup came up, or went: while it's up the grants are looked for whenever Koyo is
    /// in front, so one given some other way (the app dragged into the list), or long after
    /// the last press, is seen the moment the person is back.
    pub fn setup_shows(&mut self, open: bool, cx: &mut Context<Self>) {
        self.setup_open = open;
        if open {
            self.expect_grants(cx);
        }
    }

    /// Whether the person was sent for a grant lately.
    pub(super) fn expecting(&self) -> bool {
        (self.expected).is_some_and(|since| since.elapsed() < EXPECTED_FOR)
    }

    /// Asks a fresh process of the loader, every second, whether Screen Recording was given,
    /// for as long as that's the grant waited for; given, the host is started again.
    fn probe_along(&mut self, cx: &mut Context<Self>) {
        if self.probing.is_some() {
            return;
        }
        self.probing = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROBED_EVERY).await;
                let waits = this.update(cx, |this, cx| {
                    // With the setup up and Koyo in front, the person is at giving them:
                    // looked for from now on again, however long ago they last pressed.
                    if this.setup_open && cx.active_window().is_some() {
                        this.expected = Some(Instant::now());
                    }
                    let on = this.expecting() && this.step() != Step::Unavailable;
                    let probes = on && this.step() == Step::Needs(Grant::ScreenRecording);
                    (this.setup_open || on, probes)
                });
                let (Ok(loader), Some(dir)) = (host::loader(), desk_dir()) else {
                    break;
                };
                match waits {
                    Ok((true, true)) => {}
                    Ok((true, false)) => continue,
                    Ok((false, _)) | Err(_) => break,
                }
                let probed = cx
                    .background_executor()
                    .spawn(async move { host::probe(&loader, &dir) })
                    .await;
                let given = matches!(probed, Ok((_, true)));
                let restarted = this.update(cx, |this, cx| {
                    if given && this.step() == Step::Needs(Grant::ScreenRecording) {
                        log::info!(
                            "Screen Recording was given: starting the agent desktop's host again"
                        );
                        this.restart(cx);
                    }
                });
                if restarted.is_err() {
                    break;
                }
            }
            let _ = this.update(cx, |this, _| this.probing = None);
        }));
    }
}

/// Where the desk keeps what its host and link need.
pub(super) fn desk_dir() -> Option<std::path::PathBuf> {
    settings::data_dir().map(|data| data.join("desk"))
}
