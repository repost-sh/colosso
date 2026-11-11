//! The two privacy grants the host works by (`docs/agent-desktop.md` 11): Accessibility, to
//! make the desktop and act in its apps, and Screen Recording, to show and photograph them.
//! The host reads what it holds, and never asks: asking is done by a process of the loader
//! started for that alone ([`ask`]), when the person asked for it in Koyo.

use std::time::Instant;

use koyo_desk_wire::link::FromHost;

use crate::desktop;
use crate::host::Host;
use crate::sys;

/// The privacy grants the host holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grants {
    pub accessibility: bool,
    pub screen_recording: bool,
}

impl Grants {
    /// What the process holds, as two digits a probe's caller reads: `10` for Accessibility
    /// alone.
    pub fn said(self) -> String {
        format!(
            "{}{}",
            u8::from(self.accessibility),
            u8::from(self.screen_recording)
        )
    }

    pub fn read() -> Grants {
        // SAFETY: two plain reads of this process's grants; neither asks the person.
        unsafe {
            Grants {
                accessibility: sys::AXIsProcessTrusted() != 0,
                screen_recording: sys::CGPreflightScreenCaptureAccess(),
            }
        }
    }
}

impl Host {
    pub fn read_grants(&mut self) {
        self.grants_read = Instant::now();
        // The screen was locked when the desktop was to be made: it's unlocked now. What an
        // earlier host couldn't remove under it goes first.
        if self.desktop.is_none() && self.missing == desktop::LOCKED && !desktop::screen_locked() {
            desktop::remove_leftover_desktops(&self.state);
            self.make_desktop();
        }
        let grants = Grants::read();
        if grants == self.grants {
            return;
        }
        self.grants = grants;
        self.tell(&FromHost::Grants {
            accessibility: grants.accessibility,
            screen_recording: grants.screen_recording,
        });
        if grants.accessibility && self.desktop.is_none() {
            self.make_desktop();
        }
    }
}

/// Has macOS list this app for a grant, `accessibility` or `screen-recording`: what a process
/// of the loader started with `--ask <grant>` does, and all it does.
///
/// A process of its own, because macOS hears a process ask only once, and its helper that
/// lists an app for Screen Recording drops what it's asked without a word while it has any
/// alert up, of any app. Asked by the host that runs, a request that was dropped could never
/// be made again; asked afresh each time the person presses the button in Koyo, the next
/// press gets through. Once the app is listed, asking again does nothing.
///
/// Accessibility takes no more than checking whether the process is trusted: the first such
/// check a process makes lists its app, also after the person took it off the list. macOS's
/// own alert for it is kept out of this on purpose, being one that gets a Screen Recording
/// request dropped. Screen Recording has to be asked for, and macOS answers that with an
/// alert of its own.
pub fn ask(grant: &str) {
    // SAFETY: the first lists the app and asks nobody; the second asks the person, whose
    // answer the probes read, not this.
    unsafe {
        match grant {
            "accessibility" => {
                sys::AXIsProcessTrusted();
            }
            "screen-recording" => {
                sys::CGRequestScreenCaptureAccess();
            }
            _ => {}
        }
    }
}
