//! Files for a page: set on a file input, or given to the file chooser the page opened while
//! an agent held it, which the person is never shown.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PageOutput;
use crate::ops::shared::PaneId;
use crate::{Hints, Operation, Scope};

/// The files, and what takes them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserUploadInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The files, in your worktree: absolute, or relative to its checkout. For a chooser that
    /// asks for a folder, the one folder.
    pub paths: Vec<String>,
    /// The `<input type=file>` to set, by its ref from `browser_snapshot`, as `e12`. Left out,
    /// with no `selector` either, the files answer the file chooser the page waits on.
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    /// A CSS selector for the input, when there's no ref.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

/// `browser_upload`: files for a page.
#[derive(Debug)]
pub struct BrowserUpload;

impl Operation for BrowserUpload {
    const NAME: &'static str = "browser_upload";
    const TITLE: &'static str = "Give a page files";
    const SUBJECT: &'static [&'static str] = &["paths", "ref", "selector"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = leased!(
        "Gives a browser page files from your worktree: sets them on an `<input type=file>` (`ref` or `selector`), as if chosen there, or, with neither, answers the file chooser the page waits on (`dialog` kind `fileChooser`, which an act of yours that opens one leaves instead of the system's panel). Each path must exist in your worktree's checkout: that of the Koyo chat or terminal you run in (KOYO_CHAT, KOYO_TERMINAL), not the folder you're in; a caller in neither gives no files. browser_dialog with `dismiss` cancels the chooser instead."
    );
    const HINTS: Hints = Hints::CHANGE;
    type Input = BrowserUploadInput;
    type Output = PageOutput;

    fn summary(output: &PageOutput) -> String {
        format!("Gave files; now at {}", output.summary())
    }
}
