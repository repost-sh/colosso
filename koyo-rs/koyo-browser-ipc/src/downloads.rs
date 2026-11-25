//! What a page saves: its downloads, from where each goes to how it ended, and the page itself
//! printed or saved as a PDF (`docs/browser.md` 16).
use serde::{Deserialize, Serialize};

/// A download, as the engine says it stands ([`crate::HostEvent::Download`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    /// The host's number for it, which no other download has.
    pub id: u32,
    /// Where it comes from.
    pub url: String,
    /// The file it's saved to.
    pub path: String,
    /// How many bytes have come.
    pub received: u64,
    /// How many there are in all; `None` while the server hasn't said.
    pub total: Option<u64>,
    /// Coming, or how it ended.
    pub state: DownloadState,
}

/// Where a download is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DownloadState {
    /// Coming in.
    Going,
    /// All of it saved.
    Done,
    /// Canceled, and what had come of it gone.
    Canceled,
    /// Stopped short.
    Failed {
        /// Why, for the person.
        reason: String,
    },
}

/// What the app has the host do with a download of a page's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum DownloadAct {
    /// Save it to `path`, which the app found free or the person picked: the answer to
    /// [`crate::HostEvent::DownloadAsked`]. A file there is written over.
    SaveTo {
        /// The file, a whole path.
        path: String,
    },
    /// Cancel it: one asked about, which then never starts, or one coming in.
    Cancel,
}

/// What printing a page makes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Print {
    /// Chrome's print preview, as the page's `window.print()` opens it: the person picks a
    /// printer there, or saves a PDF.
    Preview,
    /// The page as a PDF, as it prints, at `path`: answered with
    /// [`crate::HostEvent::PdfSaved`].
    Pdf {
        /// The file, a whole path. A file there is written over.
        path: String,
    },
}
