//! What the host tells the app: pages changing, frames ready, and what each page asks.
use crate::{Dialog, MenuEntry, MenuTarget, PaneId, PixelSize};
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
    /// A pane's surface ring, sent when it opens and whenever its pixel
    /// size changes. The surfaces themselves arrive out of band, tagged
    /// with `generation`; frames of an older generation are stale.
    Surfaces {
        /// The pane.
        pane: PaneId,
        /// Bumped for every new ring.
        generation: u32,
        /// The surfaces' size: at least the page's, with room for it to
        /// grow, so a resize rarely needs a new ring.
        size: PixelSize,
        /// How many surfaces the ring has.
        slots: u8,
    },
    /// A frame is ready in `slot`. The app shows it and answers
    /// [`AppRequest::Release`] for the slot it stops showing.
    Frame {
        /// The pane.
        pane: PaneId,
        /// The ring it belongs to.
        generation: u32,
        /// The slot drawn.
        slot: u8,
        /// The page's size in the slot, from its top-left corner; the rest
        /// of the surface holds nothing to show.
        size: PixelSize,
    },
    /// The page's address, title, loading or history changed.
    Page {
        /// The pane.
        pane: PaneId,
        /// Its state.
        state: PageState,
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
    /// A request could not be carried out.
    Error {
        /// The pane it concerns, if any.
        pane: Option<PaneId>,
        /// What went wrong.
        message: String,
    },
}
