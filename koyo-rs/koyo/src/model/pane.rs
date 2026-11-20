//! A pane: what one side of a tab shows (`Content`), and the questions asked of any pane about
//! what only some kinds have, each `None` for the others.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use gpui_kit::{AnyView, App, Entity, Image, ImageFormat, SharedString};
use koyo_agent::protocol::ids::ThreadId;

use super::PaneKind;
use super::content::{Content, Device, Editor, EditorFile};
use super::page::Browser;
use crate::browser::BrowserView;
use crate::browser::profiles::By;
use crate::changes::ChangesTab;
use crate::devices::DeviceView;
use crate::files::{FilePane, file_name};

/// Ids for panes, never reused while the app runs.
static NEXT_PANE: AtomicU64 = AtomicU64::new(1);
/// Claude's logo, claude.ai's own icon: a single shape in Claude's orange.
static CLAUDE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../chat/claude.svg").to_vec();
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg))
});

#[derive(Clone, Debug)]
pub struct Pane {
    /// Which pane this is, wherever it moves.
    pub id: u64,
    pub title: SharedString,
    /// The workspace's turn when its tab was last shown, later turns higher: ⌃⇥ lists the
    /// latest first.
    pub shown_at: u64,
    /// When the person was last in it, later higher, and 0 until they have been
    /// (`Workspace::enter_pane`): of a tab's panes, the one they're in (`Tab::current`).
    pub entered_at: u64,
    pub content: Content,
}

impl Pane {
    /// An empty pane of `kind`, titled as a new tab of it.
    pub fn new(kind: PaneKind) -> Self {
        Pane::of(Content::empty(kind))
    }

    /// A pane showing `content`, titled as a new tab of its kind: `Pane { title, ..Pane::of(..) }`
    /// titles it otherwise.
    pub(crate) fn of(content: Content) -> Self {
        Pane {
            id: NEXT_PANE.fetch_add(1, Ordering::Relaxed),
            title: content.kind().new_tab_title().into(),
            shown_at: 0,
            entered_at: 0,
            content,
        }
    }

    /// What it shows: a chat, a terminal, a page, a file or the diffs, the agent desktop or a
    /// device.
    pub fn kind(&self) -> PaneKind {
        self.content.kind()
    }

    /// A terminal pane's terminal, by the number `crate::terminals` knows it by.
    pub fn terminal(&self) -> Option<u64> {
        match &self.content {
            Content::Terminal(id) => *id,
            _ => None,
        }
    }

    /// The thread a chat pane shows: a new chat has none until its first send.
    pub fn chat(&self) -> Option<&ThreadId> {
        match &self.content {
            Content::Chat(thread) => thread.as_ref(),
            _ => None,
        }
    }

    /// A device's pane showing `view`, titled as the view calls itself: the device's name, or
    /// the picker's while no device is chosen.
    pub fn device(view: Entity<DeviceView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            ..Pane::of(Content::Device(Device::Made(view)))
        }
    }

    /// A device's view, in a pane of that kind once it's made.
    pub fn device_view(&self) -> Option<&Entity<DeviceView>> {
        match &self.content {
            Content::Device(device) => device.view(),
            _ => None,
        }
    }

    /// What the pane's content adds to its header: a device's standing and buttons, as its
    /// view says.
    pub fn bar(&self, cx: &App) -> Option<AnyView> {
        self.device_view()?.read(cx).bar()
    }

    /// A browser pane showing `view`, titled as the view calls itself, an agent's when its
    /// page is.
    pub fn browser(view: Entity<BrowserView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            ..Pane::of(Content::Browser(Browser::made(view, cx)))
        }
    }

    fn page(&self) -> Option<&Browser> {
        match &self.content {
            Content::Browser(browser) => Some(browser),
            _ => None,
        }
    }

    /// Who opened a browser pane's page: an agent, when it's an agent's; every other pane is
    /// the person's.
    pub fn by(&self) -> By {
        self.page().map_or(By::Person, |browser| browser.by)
    }

    /// The page a browser pane shows, once it's made.
    pub fn view(&self) -> Option<&Entity<BrowserView>> {
        self.page()?.view()
    }

    /// The page the window's own controls act on for a browser pane: its own, or the one its
    /// `DevTools` inspect (`Shell::shown_page`).
    pub fn steered_page(&self) -> Option<&Entity<BrowserView>> {
        self.page()?.steered_page()
    }

    /// A browser pane's title, address and icon follow its page's (`BrowserEvent::Changed`):
    /// whether any of them changed.
    pub fn follow_page(&mut self, cx: &App) -> bool {
        match &mut self.content {
            Content::Browser(browser) => browser.follow(&mut self.title, cx),
            _ => false,
        }
    }

    /// The address a browser pane with no view opens once its tab is shown.
    pub fn unloaded_address(&self) -> Option<&str> {
        self.page()?.unloaded_address()
    }

    /// The history around a browser pane's page and its scroll, as the window keeps them;
    /// none for another kind.
    pub fn session(&self, cx: &App) -> koyo_browser_ipc::Session {
        self.page().map(|page| page.session(cx)).unwrap_or_default()
    }

    /// The address a browser pane shows, or opens once its tab is shown; `None` for another
    /// kind.
    pub fn address(&self) -> Option<&str> {
        self.page()?.address()
    }

    /// Whether it's a browser pane showing another page's `DevTools`.
    pub fn is_inspector(&self) -> bool {
        self.page().is_some_and(Browser::is_inspector)
    }

    /// The image a pane's tab shows for its icon: a browser pane's site's, its page's or the
    /// one kept for its address while it has no view; a chat's, Claude's logo in its colour.
    pub fn favicon(&self) -> Option<Arc<Image>> {
        match &self.content {
            Content::Browser(browser) => browser.favicon(),
            Content::Chat(_) => Some(Arc::clone(&CLAUDE)),
            Content::Terminal(_)
            | Content::Editor(_)
            | Content::Desktop(_)
            | Content::Device(_) => None,
        }
    }

    /// An editor pane showing `file`, titled with its name.
    pub fn of_file(file: Entity<FilePane>, cx: &App) -> Self {
        Pane {
            title: file.read(cx).name().into(),
            ..Pane::of(Content::Editor(Editor::File(EditorFile::Open(file))))
        }
    }

    /// An editor pane that opens the file at `path` once its tab is shown, titled with its
    /// name as it will be then.
    pub fn of_path(path: PathBuf) -> Self {
        Pane {
            title: file_name(&path).into(),
            ..Pane::of(Content::Editor(Editor::File(EditorFile::Unopened(path))))
        }
    }

    /// The file an editor pane shows, once the shell has opened one in it.
    pub fn file(&self) -> Option<&Entity<FilePane>> {
        match &self.content {
            Content::Editor(Editor::File(EditorFile::Open(file))) => Some(file),
            _ => None,
        }
    }

    /// An editor pane showing the checkout's diffs, titled for them; with none, it shows them
    /// once its tab is shown.
    pub fn of_changes(changes: Option<Entity<ChangesTab>>) -> Self {
        Pane {
            title: "Changes".into(),
            ..Pane::of(Content::Editor(Editor::Changes(changes)))
        }
    }

    /// The checkout's diffs, when it's an editor pane showing them rather than a file.
    pub fn changes(&self) -> Option<&Entity<ChangesTab>> {
        match &self.content {
            Content::Editor(Editor::Changes(changes)) => changes.as_ref(),
            _ => None,
        }
    }

    /// Whether it shows its checkout's diffs, or will once its tab is shown.
    pub fn shows_changes(&self) -> bool {
        matches!(self.content, Content::Editor(Editor::Changes(_)))
    }
}
