//! The app's browser panes, driven over CDP: listing and reading pages here, acting on them
//! in `act`. Acting needs the page's lease: a free page is claimed on the first act, and
//! `person_has_control` means the person holds the page, so the agent leaves it: for a
//! moment when they're only using it, for good when they took it.

/// The rule every acting tool's description ends with.
macro_rules! leased {
    ($($text:literal),+ $(,)?) => {
        concat!(
            $($text,)+
            " Claims a free page first. person_has_control means the person holds the page; its message says whether it comes back on its own (wait, then retry) or to open another page."
        )
    };
}

mod act;
mod viewport;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use act::{
    BrowserClaim, BrowserClaimInput, BrowserClick, BrowserClickInput, BrowserDialog,
    BrowserDialogInput, BrowserEval, BrowserEvalInput, BrowserEvalOutput, BrowserGo,
    BrowserGoInput, BrowserNavigate, BrowserNavigateInput, BrowserPressKey, BrowserPressKeyInput,
    BrowserRelease, BrowserScroll, BrowserScrollInput, BrowserType, BrowserTypeInput, DialogAction,
    GoAction, Lease, MouseButton, PageInput, WaitUntil,
};
pub use viewport::{
    BrowserViewport, BrowserViewportInput, BrowserViewportOutput, DeviceName, FitWord,
    OpenViewport, Orientation, Viewport, ViewportMode, ViewportZoom,
};

use super::shared::{PaneId, count};
use crate::{Hints, Operation, Scope};

/// How long a navigation or a wait waits when `timeoutMs` is left out, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// The longest `timeoutMs` taken, in milliseconds; a call stays well inside the client's
/// [`CALL_TIMEOUT`](crate::CALL_TIMEOUT).
pub const MAX_TIMEOUT_MS: u64 = 60_000;

/// A page and where it is, as an act leaves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageOutput {
    /// The browser pane.
    pub pane: PaneId,
    /// The page's address.
    pub url: String,
    /// The page's title.
    pub title: String,
    /// For `browser_navigate` and `browser_go`, which wait for the load: whether the page
    /// finished loading in that time. Absent when they didn't wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded: Option<bool>,
}

impl PageOutput {
    /// "“Title” (url) in pane 3".
    pub fn summary(&self) -> String {
        format!("“{}” ({}) in pane {}", self.title, self.url, self.pane)
    }
}

/// One browser page of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    /// The browser pane, which every browser tool takes as `pane`.
    pub pane: PaneId,
    /// The page's address.
    pub url: String,
    /// The page's title.
    pub title: String,
    /// Whether it's still loading.
    pub loading: bool,
    /// Whether the browser engine has it open, which the tools need. A tab brought back at
    /// launch, or one never shown, has none until Koyo's window shows it.
    #[serde(default = "super::shared::yes")]
    pub open: bool,
    /// Whether the engine has it on screen: drawn in the window, or used by an agent in the
    /// last 90 s. Out of sight its timers slow and its animation frames stop, until the next
    /// command brings it back.
    #[serde(default = "super::shared::yes")]
    pub on_screen: bool,
    /// The worktree whose tab holds it.
    pub worktree: String,
    /// Who holds its lease: an agent's name, or `person`; absent when free.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    /// The size and device it's laid out at (`browser_viewport`); `fill` when it fills its
    /// pane.
    pub viewport: Viewport,
    /// What the page asks and waits on, the first of its questions; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialog: Option<PageDialog>,
}

/// A question a page waits on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageDialog {
    /// What it is. `alert`, `confirm`, `prompt` and `leave` (the page has unsaved changes and
    /// asks before it's left) hold the page up, and `browser_dialog` answers them; `signIn`,
    /// `permission` and `certificate` are the person's to answer.
    pub kind: DialogKind,
    /// What the page says; for a sign-in, the site asking; for a permission, what it asks for.
    pub message: String,
    /// The text a prompt's field starts with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_text: Option<String>,
}

/// What a page asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    Leave,
    SignIn,
    Permission,
    Certificate,
}

impl DialogKind {
    /// Whether the page does nothing else until it's answered, which `browser_dialog` does.
    pub fn holds_up(self) -> bool {
        matches!(
            self,
            Self::Alert | Self::Confirm | Self::Prompt | Self::Leave
        )
    }
}

/// The window's pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserListOutput {
    /// Every browser pane of every worktree.
    pub pages: Vec<PageInfo>,
}

/// `browser_list`: the browser panes.
#[derive(Debug)]
pub struct BrowserList;

impl Operation for BrowserList {
    const NAME: &'static str = "browser_list";
    const TITLE: &'static str = "List browser pages";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Every browser pane in the window: pane id, address, title, whether it's loading, whether the engine has it open (the tools reach only an open page) and on screen, its worktree, who controls it, its viewport, and the dialog it waits on, if any. A page the person controls isn't yours to drive.";
    const HINTS: Hints = Hints::READ;
    type Input = super::shared::NoInput;
    type Output = BrowserListOutput;

    fn summary(output: &BrowserListOutput) -> String {
        count(output.pages.len(), "browser page", "browser pages")
    }
}

/// How `browser_read` gives the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat {
    /// The visible text.
    Text,
    /// The content as Markdown.
    Markdown,
    /// The HTML.
    Html,
}

/// What to read of a page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserReadInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `text` (the default), `markdown` or `html`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ReadFormat>,
    /// A CSS selector: only that element's content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// At most this many characters; the rest is cut and `truncated` says so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// A page's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserReadOutput {
    /// The browser pane.
    pub pane: PaneId,
    /// The page's address.
    pub url: String,
    /// The page's title.
    pub title: String,
    /// The content, in the format asked.
    pub content: String,
    /// Whether `limit` cut it.
    pub truncated: bool,
}

/// `browser_read`: a page's content.
#[derive(Debug)]
pub struct BrowserRead;

impl Operation for BrowserRead {
    const NAME: &'static str = "browser_read";
    const TITLE: &'static str = "Read a page";
    const SUBJECT: &'static [&'static str] = &["selector"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A browser page's content as text, Markdown or HTML, whole or one element's (`selector`). Reading needs no lease.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserReadInput;
    type Output = BrowserReadOutput;

    fn summary(output: &BrowserReadOutput) -> String {
        format!(
            "{} characters of “{}” ({}){}",
            output.content.chars().count(),
            output.title,
            output.url,
            if output.truncated { ", cut short" } else { "" }
        )
    }
}

/// What to snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserSnapshotInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Only elements one can click, type in or otherwise use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
}

/// A page's accessibility tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSnapshotOutput {
    /// The browser pane.
    pub pane: PaneId,
    /// The page's address.
    pub url: String,
    /// The page's title.
    pub title: String,
    /// The tree as indented text, each element one can act on marked `[ref=eN]`.
    pub tree: String,
}

/// `browser_snapshot`: the accessibility tree with refs.
#[derive(Debug)]
pub struct BrowserSnapshot;

impl Operation for BrowserSnapshot {
    const NAME: &'static str = "browser_snapshot";
    const TITLE: &'static str = "Snapshot a page";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A browser page's accessibility tree as text, elements marked `[ref=eN]` for browser_click, browser_type and browser_scroll. Refs stay valid until the next snapshot or navigation.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserSnapshotInput;
    type Output = BrowserSnapshotOutput;

    fn summary(output: &BrowserSnapshotOutput) -> String {
        format!(
            "Snapshot of “{}” ({}), {}",
            output.title,
            output.url,
            count(output.tree.lines().count(), "line", "lines")
        )
    }
}

/// The image format of a screenshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    /// The image's MIME type.
    pub fn mime_type(self) -> &'static str {
        match self {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
        }
    }
}

/// What to capture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserScreenshotInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The whole page, beyond what the pane shows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_page: Option<bool>,
    /// `png` (the default) or `jpeg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ImageFormat>,
}

/// A screenshot, which relays also give as an MCP image block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserScreenshotOutput {
    /// The browser pane.
    pub pane: PaneId,
    /// The page's address.
    pub url: String,
    /// The image's MIME type: `image/png` or `image/jpeg`.
    pub mime_type: String,
    /// The image, base64-encoded.
    pub data: String,
}

/// `browser_screenshot`: an image of a page.
#[derive(Debug)]
pub struct BrowserScreenshot;

impl Operation for BrowserScreenshot {
    const NAME: &'static str = "browser_screenshot";
    const TITLE: &'static str = "Screenshot a page";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "An image of a browser page as the pane shows it, or the whole page with `fullPage`, as PNG or JPEG. Prefer browser_snapshot or browser_read to find things on the page.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserScreenshotInput;
    type Output = BrowserScreenshotOutput;

    fn summary(output: &BrowserScreenshotOutput) -> String {
        format!(
            "Screenshot of {} ({}, {} KB)",
            output.url,
            output.mime_type,
            output.data.len() * 3 / 4 / 1024
        )
    }
}

/// What `browser_wait` waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaitFor {
    /// The page finished loading.
    Load,
    /// An element matching the CSS selector `value` exists.
    Selector,
    /// The page's text contains `value`.
    Text,
    /// The address contains `value`.
    Url,
}

/// The condition to wait for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserWaitInput {
    /// The browser pane. Left out, the page you acted on last, else the shown tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `load`, `selector`, `text` or `url`.
    #[serde(rename = "for")]
    pub until: WaitFor,
    /// The selector, text or address part `for` looks for; not for `load`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// How long to wait, in milliseconds: 30000 when left out, at most 60000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Whether the condition held in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWaitOutput {
    /// Whether it held before the time ran out.
    pub met: bool,
    /// How long the wait took, in milliseconds.
    pub elapsed_ms: u64,
}

/// `browser_wait`: until a page loads or shows something.
#[derive(Debug)]
pub struct BrowserWait;

impl Operation for BrowserWait {
    const NAME: &'static str = "browser_wait";
    const TITLE: &'static str = "Wait for a page";
    const SUBJECT: &'static [&'static str] = &["value"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Waits until a browser page loads, or has an element (`selector`), text or address part matching `value`. Running out of time answers `met: false`; a page that can't open, or a selector the page can't read, fails at once. Prefer this to fixed sleeps.";
    const HINTS: Hints = Hints::READ;
    type Input = BrowserWaitInput;
    type Output = BrowserWaitOutput;

    fn summary(output: &BrowserWaitOutput) -> String {
        if output.met {
            format!("Met after {} ms", output.elapsed_ms)
        } else {
            format!("Not met within {} ms", output.elapsed_ms)
        }
    }
}
