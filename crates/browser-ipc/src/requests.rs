//! What the app asks of the host: panes opened, sized and closed, and what the person does
//! in them.
use crate::{Answer, KeyKind, Modifiers, MouseButton, MouseKind, PaneId, Point, Viewport};
use serde::{Deserialize, Serialize};

/// A request from the app to the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppRequest {
    /// Open a pane. Its surfaces follow as [`HostEvent::Surfaces`].
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
    /// The pane changed size or moved to a display with another scale.
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
    /// The pane gained or lost keyboard focus.
    Focus {
        /// The pane.
        pane: PaneId,
        /// Whether it has focus.
        focused: bool,
    },
    /// A mouse event, at a logical position in the pane.
    Mouse {
        /// The pane.
        pane: PaneId,
        /// What happened.
        kind: MouseKind,
        /// Left edge offset.
        x: f32,
        /// Top edge offset.
        y: f32,
        /// The button pressed or released, or held while moving.
        button: Option<MouseButton>,
        /// 1 for a click, 2 for a double click.
        click_count: u32,
        /// Held modifiers.
        modifiers: Modifiers,
    },
    /// A scroll, in logical pixels.
    Wheel {
        /// The pane.
        pane: PaneId,
        /// Left edge offset of the pointer.
        x: f32,
        /// Top edge offset of the pointer.
        y: f32,
        /// Horizontal distance.
        dx: f32,
        /// Vertical distance.
        dy: f32,
        /// Held modifiers.
        modifiers: Modifiers,
    },
    /// A key event.
    Key {
        /// The pane.
        pane: PaneId,
        /// What happened.
        kind: KeyKind,
        /// The Windows virtual key code Chromium keys events by.
        key_code: i32,
        /// The platform's native key code, when known.
        native_key_code: i32,
        /// The text for [`KeyKind::Char`]; for a press or release, the characters a Mac key
        /// event for the key carries (none for a modifier key).
        text: Option<String>,
        /// Held modifiers.
        modifiers: Modifiers,
    },
    /// Input method composition.
    Ime {
        /// The pane.
        pane: PaneId,
        /// What the input method did.
        action: Ime,
    },
    /// The app is done showing `slot`; the host may draw into it again.
    Release {
        /// The pane.
        pane: PaneId,
        /// The ring the slot belongs to; a slot of an older ring is ignored.
        generation: u32,
        /// The slot.
        slot: u8,
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
            | Self::View { pane, .. }
            | Self::Mouse { pane, .. }
            | Self::Wheel { pane, .. }
            | Self::Key { pane, .. }
            | Self::Ime { pane, .. }
            | Self::Release { pane, .. }
            | Self::Edit { pane, .. }
            | Self::Answer { pane, .. }
            | Self::MenuCommand { pane, .. }
            | Self::Cdp { pane, .. } => Some(*pane),
            Self::Shutdown => None,
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

/// Input method composition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Ime {
    /// The marked text changed.
    Compose {
        /// The marked text.
        text: String,
        /// The selection in it, in UTF-16 units.
        selection: (u32, u32),
    },
    /// The text was committed.
    Commit {
        /// The committed text.
        text: String,
    },
    /// The composition was abandoned.
    Cancel,
}
