//! What the host tells the app: pages changing, and what each page asks.
use crate::{
    DevToolsAsk, Dialog, Extension, ExtensionAction, MenuEntry, MenuTarget, PaneId, PasskeyAsk,
    Permission,
};
use serde::{Deserialize, Serialize};

/// The pointer the page asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cursor {
    /// The arrow.
    #[default]
    Arrow,
    /// A link or button.
    Pointer,
    /// Text.
    Text,
    /// Busy.
    Wait,
    /// Precise selection.
    Crosshair,
    /// Not allowed.
    NotAllowed,
    /// Grab.
    Grab,
    /// Grabbing.
    Grabbing,
    /// Resize east-west.
    ResizeColumn,
    /// Resize north-south.
    ResizeRow,
    /// No cursor.
    None,
}

/// What a pane's page is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageState {
    /// The address.
    pub url: String,
    /// The title, when the page has one.
    pub title: String,
    /// Loading.
    pub loading: bool,
    /// How much of the page has loaded, in whole percents.
    pub progress: u8,
    /// History has an entry behind.
    pub can_go_back: bool,
    /// History has an entry ahead.
    pub can_go_forward: bool,
    /// The address of the page's icon, when it names one; the icon itself follows in
    /// [`HostEvent::Favicon`].
    #[serde(default)]
    pub favicon: String,
}

/// What a site was decided about one permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSetting {
    /// What it may or may not use.
    pub permission: Permission,
    /// Whether it may.
    pub allowed: bool,
}

/// An event from the host to the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostEvent {
    /// The answer to [`crate::Hello`].
    Ready {
        /// The host's [`crate::PROTOCOL`].
        protocol: u32,
        /// The engine version, as in `engine.json`.
        engine: String,
        /// The Chromium version it embeds.
        chromium: String,
    },
    /// The page's address, title, loading or history changed.
    Page {
        /// The pane.
        pane: PaneId,
        /// Its state.
        state: PageState,
    },
    /// The page's icon at `url` was fetched, through the engine and its cache.
    Favicon {
        /// The pane.
        pane: PaneId,
        /// Where the page said its icon is.
        url: String,
        /// The icon as a PNG, at most 64 pixels a side; empty when it couldn't be fetched.
        png: Vec<u8>,
    },
    /// The page went full screen, as a video's button does (`requestFullscreen`), or left it.
    Fullscreen {
        /// The pane.
        pane: PaneId,
        /// Whether it's full screen now.
        on: bool,
    },
    /// What the pane's site is allowed and blocked from, as the profile keeps it: after
    /// [`crate::AppRequest::SiteSettings`], and after each change the app makes.
    SiteSettings {
        /// The pane.
        pane: PaneId,
        /// Each permission decided for it, and whether it's allowed. One never asked for,
        /// or never answered, isn't here.
        settings: Vec<SiteSetting>,
    },
    /// The page asks for another pointer.
    Cursor {
        /// The pane.
        pane: PaneId,
        /// The pointer.
        cursor: Cursor,
    },
    /// How finding in the page went.
    Found {
        /// The pane.
        pane: PaneId,
        /// How many matches there are.
        count: u32,
        /// Which of them is selected, from 1; 0 for none.
        active: u32,
    },
    /// A pane's renderer went away (crash, out of memory, killed).
    Gone {
        /// The pane.
        pane: PaneId,
        /// Why, for the person.
        reason: String,
    },
    /// The person pressed a key in the page, which `AppKit` hands the page without the app
    /// hearing of it: said at most twice a second for a pane, and never for keys an agent sends.
    Typed {
        /// The pane.
        pane: PaneId,
    },
    /// The pane's browser is gone: the app closed it, its page closed
    /// (`DevTools` go with their page), or the host is shutting down. Its
    /// surfaces are released.
    Closed {
        /// The pane.
        pane: PaneId,
    },
    /// The pane asks for a page in a new tab: a link opened from `DevTools`.
    OpenUrl {
        /// The pane.
        pane: PaneId,
        /// The page.
        url: String,
    },
    /// A `DevTools` pane's frontend asks for something from its own controls: its dock menu,
    /// its ×, its device toolbar toggle (`docs/browser.md` 13.2).
    DevTools {
        /// The `DevTools` pane.
        pane: PaneId,
        /// What it asks.
        ask: DevToolsAsk,
    },
    /// The page or the engine asks the person something. The pane shows
    /// it and answers [`AppRequest::Answer`]; until then the page waits.
    Dialog {
        /// The pane.
        pane: PaneId,
        /// The host's number for it, unique while the host runs.
        dialog: u64,
        /// Who asks: the page's origin.
        origin: String,
        /// The question.
        question: Dialog,
    },
    /// A dialog went away without an answer: the page navigated, or the
    /// engine withdrew its question.
    DialogClosed {
        /// The pane.
        pane: PaneId,
        /// The dialog.
        dialog: u64,
    },
    /// The page wants a context menu at a logical position in the pane.
    /// The app shows it and answers [`AppRequest::MenuCommand`].
    ContextMenu {
        /// The pane.
        pane: PaneId,
        /// The host's number for it.
        menu: u64,
        /// Left edge offset.
        x: i32,
        /// Top edge offset.
        y: i32,
        /// The engine's rows.
        entries: Vec<MenuEntry>,
        /// What the menu is for.
        target: MenuTarget,
    },
    /// A page asks for a passkey, which the app gets from the system's passkey sheet and
    /// answers with [`AppRequest::Passkey`]; until then the page's promise waits. The host has
    /// checked `ask` against the frame's origin, and sends it only when the app said in
    /// [`crate::Hello`] that it can ask for passkeys.
    Passkey {
        /// The pane.
        pane: PaneId,
        /// The host's number for the request, unique while the host runs.
        request: u64,
        /// The origin of the frame that asks, as the engine knows it: `https://example.com`.
        origin: String,
        /// What it asks for.
        ask: Box<PasskeyAsk>,
    },
    /// A passkey request went away without an answer: the page aborted it, navigated or
    /// closed. The app takes its sheet down; an answer that comes anyway is dropped.
    PasskeyWithdrawn {
        /// The pane.
        pane: PaneId,
        /// The request.
        request: u64,
    },
    /// The answer to [`AppRequest::Cdp`].
    CdpReply {
        /// The page.
        pane: PaneId,
        /// The app's number for the call.
        call: u64,
        /// The command's result, when it succeeded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        /// Why it failed, when it did: the protocol's error message, or the
        /// host's when the page is gone or the result was too large to send.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// An event of the page's own `DevTools` protocol session, once the app
    /// has sent it a command.
    CdpEvent {
        /// The page.
        pane: PaneId,
        /// The event, such as `Runtime.consoleAPICalled`.
        method: String,
        /// Its parameters.
        params: serde_json::Value,
    },
    /// The extensions installed in a profile (`docs/extensions.md`): once the engine has
    /// the profile open, and whenever one is installed, removed, turned on or off, updated
    /// or pinned. A profile with none is told so, with none: that it's told at all says the
    /// engine runs extensions with their icons and popups.
    Extensions {
        /// The profile, as [`AppRequest::Open`] names it.
        profile: String,
        /// Its extensions, by name.
        extensions: Vec<Extension>,
    },
    /// What an extension set its toolbar action to (`chrome.action.setIcon`, `setTitle`,
    /// `setBadgeText`, `disable`): for one page, or for every page of the profile.
    ExtensionAction {
        /// The profile.
        profile: String,
        /// The extension's id.
        extension: String,
        /// The page it's for; `None` for every page.
        pane: Option<PaneId>,
        /// What it's set to.
        action: ExtensionAction,
    },
    /// An extension opened a tab (`chrome.tabs.create`, its options, its welcome page). Its
    /// page is made already, as pane `pane`, a number the host gave it: the app gives it a
    /// tab, and sizes and shows it as any pane, without opening it.
    TabOpened {
        /// The new page's pane.
        pane: PaneId,
        /// The pane whose profile it's of, and beside whose tab it belongs.
        opener: PaneId,
        /// The page it loads.
        url: String,
        /// Whether the extension asked for it in front.
        active: bool,
    },
    /// An extension asked for the pane's tab to be brought forward
    /// (`chrome.tabs.update` with `active`).
    TabActivated {
        /// The pane.
        pane: PaneId,
    },
    /// A request could not be carried out.
    Error {
        /// The pane it concerns, if any.
        pane: Option<PaneId>,
        /// What went wrong.
        message: String,
    },
}
