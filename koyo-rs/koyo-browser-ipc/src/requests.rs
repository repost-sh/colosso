//! What the app asks of the host: panes opened, sized and closed, and what the person asks
//! of their pages. A page's pointer, wheel and keys are `AppKit`'s to hand it, not the app's.
use crate::{
    Answer, DevToolsDock, ExtensionChange, PaneId, PasskeyAnswer, Permission, Point, Session,
    Viewport,
};
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
    /// The page `pane` opens next ([`Self::Open`], sent just after) is one brought back, with
    /// the history and scroll it had: it opens with that history behind and ahead of it,
    /// scrolled where it was.
    BringBack {
        /// The pane.
        pane: PaneId,
        /// Its history and scroll.
        session: Session,
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
    /// Close the pane and release its surfaces. The app let the page go: one that asks to
    /// confirm leaving isn't asked, and goes all the same.
    Close {
        /// The pane.
        pane: PaneId,
    },
    /// The person closes the page, which is asked first, as Chrome asks it: one that asks to
    /// confirm leaving asks the person ([`crate::HostEvent::Dialog`] with
    /// [`crate::Dialog::BeforeUnload`]). The host says [`crate::HostEvent::Closed`] once the
    /// page went, and nothing when the person stays; a [`Self::Close`] meanwhile lets it go.
    Leave {
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
    /// What the app knows of whether the page is to be seen: the host knows itself whether a
    /// pane draws it (`docs/native-pages.md` 5). The page is on screen for the engine while a
    /// pane draws it in a window that's `seen`, or while it's `kept`; out of sight it stops
    /// painting.
    Sight {
        /// The pane.
        pane: PaneId,
        /// Whether an agent keeps the page on screen for the engine, a pane drawing it or not.
        kept: bool,
        /// Whether the window the page's pane draws it in counts as seen.
        seen: bool,
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
    /// own), or in `session`, one of a frame attached under it. Answered with
    /// [`HostEvent::CdpReply`] for `call`. An agent's `<Domain>.enable` has the
    /// page's events of that domain come as [`HostEvent::CdpEvent`], until its
    /// `<Domain>.disable`; its `Target.setAutoAttach` so has the `Target` domain's.
    Cdp {
        /// The page.
        pane: PaneId,
        /// The app's number for the call, echoed in the reply.
        call: u64,
        /// Whose command it is.
        caller: CdpCaller,
        /// The session of a frame attached under the page's own
        /// (`Target.attachedToTarget`), for a command to run there; none for the page's own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        /// The protocol method, such as `Runtime.evaluate`.
        method: String,
        /// Its parameters: a JSON object.
        #[serde(default)]
        params: serde_json::Value,
    },
    /// The pointer went down on an extension's icon for the pane's page; the press it starts
    /// follows as [`AppRequest::ExtensionAct`] (`docs/extensions.md` 6). The extension's popup
    /// open over the page closes, and that press opens none.
    ExtensionPointerDown {
        /// The page.
        pane: PaneId,
        /// The extension's id.
        extension: String,
    },
    /// A press on an extension's icon for the pane's page, as on its button in Chrome's
    /// toolbar (`docs/extensions.md` 6): its open popup closes, or its action runs, opening
    /// its popup or telling it `action.onClicked`, with `activeTab` for the page either way.
    ExtensionAct {
        /// The page.
        pane: PaneId,
        /// The extension's id.
        extension: String,
        /// The icon pressed, which its popup hangs from.
        bounds: crate::ScreenRect,
        /// Which of the popup's top corners is at the same corner of the icon.
        #[serde(default)]
        corner: crate::Corner,
    },
    /// Turn an extension of a profile on or off, pin it, or remove it. The host says how
    /// the profile's extensions stand after ([`crate::HostEvent::Extensions`]).
    ChangeExtension {
        /// The profile, as [`AppRequest::Open`] names it.
        profile: String,
        /// The extension's id.
        extension: String,
        /// What to do with it.
        change: ExtensionChange,
    },
    /// The person is on this page: the one with the keyboard, or shown last. Extensions get
    /// it as the current window's active tab.
    Active {
        /// The page.
        pane: PaneId,
    },
    /// Print the page: Chrome's print preview, or the page as a PDF.
    Print {
        /// The page.
        pane: PaneId,
        /// What printing it makes.
        print: crate::Print,
    },
    /// Where a download of the page's goes ([`crate::HostEvent::DownloadAsked`]), or that it's
    /// canceled.
    Download {
        /// The page that started it.
        pane: PaneId,
        /// The engine's number for it.
        download: u32,
        /// What's done with it.
        act: crate::DownloadAct,
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
            | Self::BringBack { pane, .. }
            | Self::OpenDevTools { pane, .. }
            | Self::Resize { pane, .. }
            | Self::Navigate { pane, .. }
            | Self::Go { pane, .. }
            | Self::Close { pane }
            | Self::Leave { pane }
            | Self::Sight { pane, .. }
            | Self::ExitFullscreen { pane }
            | Self::SiteSettings { pane }
            | Self::SetSiteSetting { pane, .. }
            | Self::View { pane, .. }
            | Self::Edit { pane, .. }
            | Self::Answer { pane, .. }
            | Self::Passkey { pane, .. }
            | Self::MenuCommand { pane, .. }
            | Self::ExtensionPointerDown { pane, .. }
            | Self::ExtensionAct { pane, .. }
            | Self::Active { pane }
            | Self::Print { pane, .. }
            | Self::Download { pane, .. }
            | Self::Cdp { pane, .. } => Some(*pane),
            Self::ChangeExtension { .. } | Self::ClearData | Self::Shutdown => None,
        }
    }
}

/// Whose a protocol command on a page is ([`AppRequest::Cdp`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CdpCaller {
    /// The app's own, for the person: a page's history read, a screenshot taken. It hears
    /// its answer and nothing else.
    App,
    /// An agent's, through the browser tools: the domains it enables send their events.
    Agent,
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

