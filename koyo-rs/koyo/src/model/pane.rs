//! A pane: what one side of a tab shows, by its kind. A page, or the address it opens once its
//! tab is shown; a file or the checkout's diffs; a terminal or a chat, by what knows them; the
//! agent desktop's view, or a device's.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use gpui_kit::{AnyView, App, Entity, Image, ImageFormat, SharedString};
use koyo_agent::protocol::ids::ThreadId;

use super::PaneKind;
use crate::agent_desktop::DeskView;
use crate::browser::BrowserView;
use crate::changes::ChangesTab;
use crate::devices::DeviceView;
use crate::files::FilePane;

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
    pub kind: PaneKind,
    pub title: SharedString,
    /// A browser pane's page, or what it opens once its tab is shown (`view`, `set_view`);
    /// the agent desktop's view in a pane of that kind (`desktop`), a device's in a device's
    /// pane (`device`).
    pub(super) page: Option<Page>,
    /// The file an editor pane shows, once the shell has opened one in it.
    pub file: Option<Entity<FilePane>>,
    /// The checkout's diffs, when it's an editor pane showing them rather than a file.
    pub changes: Option<Entity<ChangesTab>>,
    /// What an editor pane brought back with the window shows once its tab is (`unopened.rs`).
    /// Boxed, as few panes have one.
    pub unopened: Option<Box<super::Unopened>>,
    /// The workspace's turn when its tab was last shown, later turns higher: ⌃⇥ lists the
    /// latest first.
    pub shown_at: u64,
    /// When the person was last in it, later higher, and 0 until they have been
    /// (`Workspace::enter_pane`): of a tab's panes, the one they're in (`Tab::current`).
    pub entered_at: u64,
    /// A terminal pane's terminal, by the number `crate::terminals` knows it by.
    pub terminal: Option<u64>,
    /// The thread a chat pane shows: a new chat has none until its first send.
    pub chat: Option<ThreadId>,
}

impl Pane {
    /// An empty pane of `kind`, titled as a new tab of it.
    pub fn new(kind: PaneKind) -> Self {
        Pane {
            id: NEXT_PANE.fetch_add(1, Ordering::Relaxed),
            kind,
            title: kind.new_tab_title().into(),
            page: None,
            file: None,
            changes: None,
            unopened: None,
            shown_at: 0,
            entered_at: 0,
            terminal: None,
            chat: None,
        }
    }

    /// A pane showing the agent desktop.
    pub fn desktop(view: Entity<DeskView>) -> Self {
        Pane {
            page: Some(Page::Desktop(view)),
            ..Pane::new(PaneKind::Desktop)
        }
    }

    /// The pane shows the agent desktop through `view` from now on.
    pub fn set_desktop(&mut self, view: Entity<DeskView>) {
        self.page = Some(Page::Desktop(view));
    }

    /// The agent desktop's view, in a pane of that kind.
    pub fn desktop_view(&self) -> Option<&Entity<DeskView>> {
        match &self.page {
            Some(Page::Desktop(view)) => Some(view),
            Some(Page::Made(_) | Page::Device(_) | Page::Unloaded(_)) | None => None,
        }
    }

    /// A device's pane showing `view`, titled as the view calls itself: the device's name, or
    /// the picker's while no device is chosen.
    pub fn device(view: Entity<DeviceView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            page: Some(Page::Device(Box::new(Device::of(view, cx)))),
            ..Pane::new(PaneKind::Device)
        }
    }

    /// The pane shows `view` from now on, titled as it calls itself.
    pub fn set_device(&mut self, view: Entity<DeviceView>, cx: &App) {
        self.title = view.read(cx).title().into();
        self.page = Some(Page::Device(Box::new(Device::of(view, cx))));
    }

    /// A device's view, in a pane of that kind.
    pub fn device_view(&self) -> Option<&Entity<DeviceView>> {
        match &self.page {
            Some(Page::Device(device)) => Some(&device.view),
            Some(Page::Made(_) | Page::Desktop(_) | Page::Unloaded(_)) | None => None,
        }
    }

    /// What the pane's content adds to its header: a device's standing and buttons. Kept in
    /// the pane as the title is, said by the view when it changes ([`Pane::set_bar`]), so the
    /// window doesn't read the view as it draws: it would draw again with each of the device's
    /// pictures.
    pub fn bar(&self) -> Option<AnyView> {
        match &self.page {
            Some(Page::Device(device)) => device.bar.clone(),
            Some(Page::Made(_) | Page::Desktop(_) | Page::Unloaded(_)) | None => None,
        }
    }

    /// A device's pane has `bar` in its header from now on; no other kind keeps one.
    pub fn set_bar(&mut self, bar: Option<AnyView>) {
        if let Some(Page::Device(device)) = &mut self.page {
            device.bar = bar;
        }
    }

    /// A browser pane showing `view`, titled as the view calls itself.
    pub fn browser(view: Entity<BrowserView>, cx: &App) -> Self {
        Pane {
            title: view.read(cx).title().into(),
            page: Some(Page::Made(view)),
            ..Pane::new(PaneKind::Browser)
        }
    }

    /// A browser pane with no view yet, titled `title`, that opens `unloaded`'s address once
    /// its tab is shown.
    pub fn unloaded(unloaded: Unloaded, title: SharedString) -> Self {
        Pane {
            title,
            page: Some(Page::Unloaded(Box::new(unloaded))),
            ..Pane::new(PaneKind::Browser)
        }
    }

    /// The page a browser pane shows, once it's made.
    pub fn view(&self) -> Option<&Entity<BrowserView>> {
        match &self.page {
            Some(Page::Made(view)) => Some(view),
            Some(Page::Unloaded(_) | Page::Desktop(_) | Page::Device(_)) | None => None,
        }
    }

    /// The pane shows `view` from now on, and no longer holds an address to open.
    pub fn set_view(&mut self, view: Entity<BrowserView>) {
        self.page = Some(Page::Made(view));
    }

    /// A browser pane's page is closed: it holds what it opens once its tab is shown again, as
    /// a pinned page does once it's closed (`docs/arc-sidebar.md` 11).
    pub fn unload(&mut self, unloaded: Unloaded) {
        self.page = Some(Page::Unloaded(Box::new(unloaded)));
    }

    /// The address a browser pane with no view opens once its tab is shown.
    pub fn unloaded_address(&self) -> Option<&str> {
        match &self.page {
            Some(Page::Unloaded(unloaded)) => Some(&unloaded.address),
            Some(Page::Made(_) | Page::Desktop(_) | Page::Device(_)) | None => None,
        }
    }

    /// The address a browser pane shows, or opens once its tab is shown; `None` for another
    /// kind.
    pub fn address(&self, cx: &App) -> Option<String> {
        match &self.page {
            Some(Page::Made(view)) => Some(view.read(cx).address()),
            Some(Page::Unloaded(unloaded)) => Some(unloaded.address.clone()),
            Some(Page::Desktop(_) | Page::Device(_)) | None => None,
        }
    }

    /// Whether it's a browser pane showing another page's `DevTools`.
    pub fn is_inspector(&self, cx: &App) -> bool {
        self.view().is_some_and(|view| view.read(cx).is_inspector())
    }

    /// The image a pane's tab shows for its icon: a browser pane's site's, its page's or the
    /// one kept for its address while it has no view; a chat's, Claude's logo in its colour.
    pub fn favicon(&self, cx: &App) -> Option<Arc<Image>> {
        match &self.page {
            Some(Page::Made(view)) => view.read(cx).favicon(),
            Some(Page::Unloaded(unloaded)) => unloaded.favicon.clone(),
            Some(Page::Desktop(_) | Page::Device(_)) => None,
            None => (self.kind == PaneKind::Chat).then(|| Arc::clone(&CLAUDE)),
        }
    }

    /// An editor pane showing `file`, titled with its name.
    pub fn file(file: Entity<FilePane>, cx: &App) -> Self {
        Pane {
            title: file.read(cx).name().into(),
            file: Some(file),
            ..Pane::new(PaneKind::Editor)
        }
    }

    /// An editor pane showing the checkout's diffs, titled for them.
    pub fn changes(changes: Entity<ChangesTab>) -> Self {
        Pane {
            title: "Changes".into(),
            changes: Some(changes),
            ..Pane::new(PaneKind::Editor)
        }
    }
}

/// A device's view, and its bar for the pane's header.
#[derive(Clone, Debug)]
pub(super) struct Device {
    view: Entity<DeviceView>,
    bar: Option<AnyView>,
}

impl Device {
    fn of(view: Entity<DeviceView>, cx: &App) -> Self {
        let bar = view.read(cx).bar();
        Device { view, bar }
    }
}

/// A browser pane's page: made, or what it opens once its tab is shown, never both. Or, in
/// the agent desktop's pane, its view; in a device's pane, the device's view.
#[derive(Clone, Debug)]
pub(super) enum Page {
    Made(Entity<BrowserView>),
    Desktop(Entity<DeskView>),
    /// Boxed, as few panes are a device's.
    Device(Box<Device>),
    /// A tab brought back at launch, its page made when the tab is first shown
    /// (`docs/browser.md` 5). Boxed, as few panes have one.
    Unloaded(Box<Unloaded>),
}

/// What a browser pane with no view yet opens once its tab is shown.
#[derive(Clone, Debug)]
pub struct Unloaded {
    pub address: String,
    /// The icon kept for the address's site, shown on the tab meanwhile.
    pub favicon: Option<Arc<Image>>,
}
