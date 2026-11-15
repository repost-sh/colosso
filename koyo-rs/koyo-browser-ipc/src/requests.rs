//! What the app asks of the host: panes opened, sized and closed, and what the person asks
//! of their pages. A page's pointer, wheel and keys are `AppKit`'s to hand it, not the app's.
use crate::{Answer, DevToolsDock, PaneId, PasskeyAnswer, Permission, Point, Viewport};
use serde::{Deserialize, Serialize};

/// A request from the app to the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppRequest {
    /// Open a pane.
    Open {
        /// The app's id for the pane.
        pane: PaneId,
        /// The page to load.
        url: String,
        /// The pane's size.
        viewport: Viewport,
        /// The profile (cookies, storage, cache) the pane uses; one per
        /// project, so projects never share logins.
        profile: String,
    },
    /// The size of a page no pane draws, as an agent sizes one out of sight. A page a pane
    /// draws is the size of the pane.
    Resize {
        /// The pane.
        pane: PaneId,
        /// Its new size.
        viewport: Viewport,
    },
    /// Load a page.
    Navigate {
        /// The pane.
        pane: PaneId,
        /// The page.
        url: String,
    },
    /// History and loading controls.
    Go {
        /// The pane.
        pane: PaneId,
        /// What to do.
        to: Go,
    },
    /// Close the pane and release its surfaces.
    Close {
        /// The pane.
        pane: PaneId,
    },
    /// How the page is looked at: zoomed, or searched for text.
    View {
        /// The pane.
        pane: PaneId,
        /// What changes.
        action: ViewAction,
    },
    /// The pane came into sight or went out of it: a page out of sight stops painting.
    Visible {
        /// The pane.
        pane: PaneId,
        /// Whether it's on screen.
        visible: bool,
    },
    /// The page is to leave full screen: the person pressed Escape, or took the window out
    /// of it.
    ExitFullscreen {
        /// The pane.
        pane: PaneId,
    },
    /// What the pane's site is allowed and blocked from is wanted
    /// ([`crate::HostEvent::SiteSettings`]).
    SiteSettings {
        /// The pane.
        pane: PaneId,
    },
    /// The pane's site is allowed `permission` from now on, or blocked from it. The host says
    /// how its settings stand after ([`crate::HostEvent::SiteSettings`]).
    SetSiteSetting {
        /// The pane.
        pane: PaneId,
        /// What it may or may not use.
        permission: Permission,
        /// Whether it may.
        allowed: bool,
    },
    /// The pane gained or lost keyboard focus.
    Focus {
        /// The pane.
        pane: PaneId,
        /// Whether it has focus.
        focused: bool,
    },
    /// An editing command for the focused element of the page.
    Edit {
        /// The pane.
        pane: PaneId,
        /// The command.
        command: Edit,
    },
    /// The person's answer to a [`HostEvent::Dialog`].
    Answer {
        /// The pane.
        pane: PaneId,
        /// The dialog, as the host numbered it.
        dialog: u64,
        /// The answer.
        answer: Answer,
    },
    /// The answer to a [`HostEvent::Passkey`].
    Passkey {
        /// The pane.
        pane: PaneId,
        /// The request, as the host numbered it.
        request: u64,
        /// The passkey, or why there's none.
        answer: PasskeyAnswer,
    },
    /// What the person picked from a [`HostEvent::ContextMenu`].
    MenuCommand {
        /// The pane.
        pane: PaneId,
        /// The menu, as the host numbered it.
        menu: u64,
        /// The engine's command, or `None` when the menu closed without one
        /// (or the app carried out an item of its own).
        command: Option<i32>,
    },
    /// Open the Chrome `DevTools` of `pane`'s page as a pane of its own, under
    /// the id `devtools`: it paints into surfaces of its own, takes input
    /// and closes like any pane, and goes when its page closes
    /// ([`HostEvent::Closed`]). When the page's `DevTools` are open already,
    /// this only inspects the element at `inspect`.
    OpenDevTools {
        /// The page.
        pane: PaneId,
        /// The app's id for the `DevTools` pane.
        devtools: PaneId,
        /// The `DevTools` pane's size.
        viewport: Viewport,
        /// An element of the page to inspect, at a logical position in the
        /// page's pane.
        inspect: Option<Point>,
        /// The side the frontend opens at, which its dock menu shows checked.
        #[serde(default)]
        dock: DevToolsDock,
    },
    /// Run a Chrome `DevTools` protocol command on `pane`'s page, in the
    /// page's own session (an open `DevTools` frontend has a session of its
    /// own). Answered with [`HostEvent::CdpReply`] for `call`. From the
    /// first command on, the page's events in that session come as
    /// [`HostEvent::CdpEvent`].
    Cdp {
        /// The page.
        pane: PaneId,
        /// The app's number for the call, echoed in the reply.
        call: u64,
        /// The protocol method, such as `Runtime.evaluate`.
        method: String,
        /// Its parameters: a JSON object.
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Forget what every profile keeps (cookies, sign-ins, caches, site storage) without
    /// stopping: every pane closes ([`HostEvent::Closed`]) and a page opened from then on is of
    /// a fresh profile. For a host in the app's own process, which can't be stopped and
    /// started again.
    ClearData,
    /// Quit the host.
    Shutdown,
}

impl AppRequest {
    /// The pane a request is for; `None` for the whole host.
    #[must_use]
    pub fn pane(&self) -> Option<PaneId> {
        match self {
            Self::Open { pane, .. }
            | Self::OpenDevTools { pane, .. }
            | Self::Resize { pane, .. }
            | Self::Navigate { pane, .. }
            | Self::Go { pane, .. }
            | Self::Close { pane }
            | Self::Focus { pane, .. }
            | Self::Visible { pane, .. }
            | Self::ExitFullscreen { pane }
            | Self::SiteSettings { pane }
            | Self::SetSiteSetting { pane, .. }
            | Self::View { pane, .. }
            | Self::Edit { pane, .. }
            | Self::Answer { pane, .. }
            | Self::Passkey { pane, .. }
            | Self::MenuCommand { pane, .. }
            | Self::Cdp { pane, .. } => Some(*pane),
            Self::ClearData | Self::Shutdown => None,
        }
    }
}

/// How a page is looked at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ViewAction {
    /// Zooms to `level`: 0 is the page's own size, each step about a fifth larger or smaller.
    Zoom {
        /// The zoom level.
        level: f64,
    },
    /// Finds `text`, highlighting every match and selecting one: the first, for text not
    /// just found; otherwise the one after the match selected, or before it.
    Find {
        /// What to find.
        text: String,
        /// Down the page, or up it.
        forward: bool,
    },
    /// Stops finding, the match selected staying selected unless `clear`.
    StopFind {
        /// Whether the selection goes too.
        clear: bool,
    },
}

/// History and loading controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Go {
    /// Back in history.
    Back,
    /// Forward in history.
    Forward,
    /// Reload the page.
    Reload,
    /// Stop loading.
    Stop,
}

/// Editing commands, as in a browser's Edit menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edit {
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Cut the selection.
    Cut,
    /// Copy the selection.
    Copy,
    /// Paste.
    Paste,
    /// Select everything.
    SelectAll,
}

