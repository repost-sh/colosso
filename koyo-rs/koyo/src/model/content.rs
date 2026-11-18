//! What a pane shows, one value per kind: the kind is which variant it is, and the variant
//! holds that kind's state and nothing else's.

use std::path::PathBuf;

use gpui_kit::{App, Entity};
use koyo_agent::protocol::ids::ThreadId;

use super::PaneKind;
use super::page::Browser;
use crate::agent_desktop::DeskView;
use crate::changes::ChangesTab;
use crate::devices::{Chosen, DeviceView};
use crate::files::FilePane;

/// What a pane shows.
#[derive(Clone, Debug)]
pub enum Content {
    /// The thread it shows: a new chat has none until its first send.
    Chat(Option<ThreadId>),
    /// Its terminal, by the number `crate::terminals` knows it by.
    Terminal(Option<u64>),
    Browser(Browser),
    Editor(Editor),
    /// The agent desktop's view (`docs/agent-desktop.md` 9); none until its tab is shown, for
    /// one closed and brought back.
    Desktop(Option<Entity<DeskView>>),
    Device(Device),
}

impl Content {
    /// Which kind of pane shows it.
    pub fn kind(&self) -> PaneKind {
        match self {
            Content::Chat(_) => PaneKind::Chat,
            Content::Terminal(_) => PaneKind::Terminal,
            Content::Browser(_) => PaneKind::Browser,
            Content::Editor(_) => PaneKind::Editor,
            Content::Desktop(_) => PaneKind::Desktop,
            Content::Device(_) => PaneKind::Device,
        }
    }

    /// What a new pane of `kind` shows: a chat with no thread, a terminal with none, the start
    /// page, an untitled editor, the desktop or the device picker with no view yet.
    pub fn empty(kind: PaneKind) -> Self {
        match kind {
            PaneKind::Chat => Content::Chat(None),
            PaneKind::Terminal => Content::Terminal(None),
            PaneKind::Browser => Content::Browser(Browser::start()),
            PaneKind::Editor => Content::Editor(Editor::Untitled),
            PaneKind::Desktop => Content::Desktop(None),
            PaneKind::Device => Content::Device(Device::Unmade(None)),
        }
    }

    /// Lets go of its views, for a tab kept closed, keeping what makes them again once the tab
    /// is back and shown (`Shell::make_shown`): a page its address, its icon and whose it is, a
    /// file its path, a device the device it shows; the diffs and the desktop are made anew.
    pub(super) fn let_go(&mut self, cx: &App) {
        match self {
            Content::Chat(_) | Content::Terminal(_) | Content::Editor(Editor::Untitled) => {}
            Content::Browser(browser) => browser.let_go(),
            Content::Editor(Editor::File(file)) => *file = EditorFile::Unopened(file.path(cx)),
            Content::Editor(Editor::Changes(changes)) => *changes = None,
            Content::Desktop(view) => *view = None,
            Content::Device(device) => *device = Device::Unmade(device.chosen(cx)),
        }
    }
}

/// What an editor pane shows: nothing, a file, or the checkout's diffs.
#[derive(Clone, Debug)]
pub enum Editor {
    Untitled,
    File(EditorFile),
    /// The checkout's diffs, made over its panels, which only the selected checkout has; none
    /// until its tab is shown, for one brought back with the window or with its tab.
    Changes(Option<Entity<ChangesTab>>),
}

/// The file an editor pane shows: open, or, brought back with the window (`docs/browser.md`
/// 4) or with its tab, its path, opened once its tab is shown as a page brought back opens its
/// address then.
#[derive(Clone, Debug)]
pub enum EditorFile {
    Open(Entity<FilePane>),
    Unopened(PathBuf),
}

impl EditorFile {
    /// The file's path, open or not.
    pub fn path(&self, cx: &App) -> PathBuf {
        match self {
            EditorFile::Open(file) => file.read(cx).path.clone(),
            EditorFile::Unopened(path) => path.clone(),
        }
    }
}

/// What a device's pane shows (`docs/devices.md`): its view, a device's or the picker's until
/// one is chosen; or, for one closed and brought back, the device it shows once its tab is
/// shown, none for the picker.
#[derive(Clone, Debug)]
pub enum Device {
    Made(Entity<DeviceView>),
    Unmade(Option<Chosen>),
}

impl Device {
    /// Its view, once it's made.
    pub fn view(&self) -> Option<&Entity<DeviceView>> {
        match self {
            Device::Made(view) => Some(view),
            Device::Unmade(_) => None,
        }
    }

    /// The device it shows; none for the picker.
    pub fn chosen(&self, cx: &App) -> Option<Chosen> {
        match self {
            Device::Made(view) => view.read(cx).chosen().cloned(),
            Device::Unmade(chosen) => chosen.clone(),
        }
    }
}
