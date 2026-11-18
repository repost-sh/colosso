//! What the shown tab's panes show but don't have yet, made once the tab is shown and before
//! it's drawn: a terminal's or a chat's view, a page at the address it kept, a file opened as it
//! is on disk, the checkout's diffs, the agent desktop, a device or the picker. A pane brought
//! back with the window, or with its tab by ⌘⇧T (`Content::let_go`), holds what it makes then.

use std::path::PathBuf;

use gpui_kit::{AppContext as _, Context, Window};
use koyo_agent::protocol::ids::ThreadId;

use super::Shell;
use crate::agent_desktop::DeskView;
use crate::browser::profiles::By;
use crate::devices::Chosen;
use crate::model::{Content, Device, Editor, EditorFile, Tab, Unloaded};

/// What the shown tab's panes don't have yet, by their panes' ids.
#[derive(Default)]
struct Unmade {
    terminals: Vec<u64>,
    /// The chat panes without their views, and the threads they show; none when no pane is a
    /// chat's.
    chats: Option<Vec<(u64, Option<ThreadId>)>>,
    pages: Vec<(u64, Unloaded, By)>,
    files: Vec<(u64, PathBuf)>,
    changes: Vec<u64>,
    desktops: Vec<u64>,
    devices: Vec<(u64, Option<Chosen>)>,
}

impl Shell {
    /// Makes what the shown tab's panes show but don't have yet, so the panes draw it: as the
    /// shell changes and as it's drawn, and where a pane must have it at once, asked for or
    /// back with ⌘⇧T.
    pub(crate) fn make_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let unmade = self.unmade();
        for id in unmade.terminals {
            self.terminal_view(id, window, cx);
        }
        if let Some(chats) = unmade.chats {
            self.make_chats(chats, window, cx);
        }
        self.make_pages(unmade.pages, window, cx);
        if let Some(at) = self.workspace.shown_checkout_tab() {
            self.reopen_files(at, &unmade.files, window, cx);
            self.reopen_changes(at, &unmade.changes, window, cx);
        }
        for id in unmade.desktops {
            let view = cx.new(DeskView::new);
            let pane = self.workspace.pane_mut(id);
            if let Some(Content::Desktop(desktop)) = pane.map(|pane| &mut pane.content) {
                *desktop = Some(view);
            }
        }
        for (id, chosen) in unmade.devices {
            let view = self.device_view(chosen, cx);
            let pane = self.workspace.pane_mut(id);
            if let Some(Content::Device(device)) = pane.map(|pane| &mut pane.content) {
                *device = Device::Made(view);
            }
        }
    }

    /// What the shown tab's panes show but don't have yet, each kind saying what it makes.
    fn unmade(&self) -> Unmade {
        let mut unmade = Unmade::default();
        for pane in self.workspace.tab_shown().map(Tab::panes).unwrap_or_default() {
            let id = pane.id;
            match &pane.content {
                Content::Chat(thread) => {
                    let chats = unmade.chats.get_or_insert_default();
                    if self.chat_tabs.view(id).is_none() {
                        chats.push((id, thread.clone()));
                    }
                }
                Content::Terminal(terminal) => unmade.terminals.extend(*terminal),
                Content::Browser(browser) => {
                    let page = browser.unloaded().map(|page| (id, page.clone(), browser.by));
                    unmade.pages.extend(page);
                }
                Content::Editor(Editor::File(EditorFile::Unopened(path))) => {
                    unmade.files.push((id, path.clone()));
                }
                Content::Editor(Editor::Changes(None)) => unmade.changes.push(id),
                Content::Desktop(None) => unmade.desktops.push(id),
                Content::Device(Device::Unmade(chosen)) => {
                    unmade.devices.push((id, chosen.clone()));
                }
                Content::Editor(
                    Editor::Untitled | Editor::File(EditorFile::Open(_)) | Editor::Changes(Some(_)),
                )
                | Content::Desktop(Some(_))
                | Content::Device(Device::Made(_)) => {}
            }
        }
        unmade
    }
}
