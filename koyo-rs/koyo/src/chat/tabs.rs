//! Chat tabs on the shell (`docs/chat.md` 5.2). A chat pane's view is made when it's first shown.
//! Which chats have a tab follows from what the panes show, whichever way a tab came or went,
//! and the host is told, so the tabs come back with the app.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use koyo_agent::protocol::ids::ThreadId;

use super::view::{ChatView, ChatViewEvent};
use super::{Chats, ChatsEvent};
use crate::model::{PaneKind, Workspace};
use crate::shell::Shell;

pub struct ChatTabs {
    pub(super) chats: Entity<Chats>,
    /// Each chat pane's view once it's shown, by the pane's id.
    views: HashMap<u64, Made>,
    /// The threads the panes show.
    shown: HashSet<ThreadId>,
    /// The pane whose view takes the keyboard once it's made: the chat just opened.
    focus_next: Option<u64>,
    _followed: Subscription,
}

struct Made {
    view: Entity<ChatView>,
    _events: Subscription,
}

impl ChatTabs {
    pub fn new(cx: &mut Context<Shell>) -> Self {
        let chats = cx.new(Chats::new);
        let _followed = cx.subscribe(&chats, |this, _, event, cx| match event {
            ChatsEvent::Summaries => this.follow_chats(cx),
            ChatsEvent::Removed(id) => this.close_chat_tabs(id, cx),
        });
        ChatTabs {
            chats,
            views: HashMap::new(),
            shown: HashSet::new(),
            focus_next: None,
            _followed,
        }
    }

    /// The view of chat pane `pane`, once it's made.
    pub fn view(&self, pane: u64) -> Option<&Entity<ChatView>> {
        self.views.get(&pane).map(|made| &made.view)
    }

    /// Follows what the panes show, after any change to them: the views of panes gone are let
    /// go, and the host hears of a thread no pane shows now or one shown again. It's told even
    /// if its last word says so already, as that word may be about to change.
    pub fn retain(&mut self, workspace: &Workspace, cx: &mut Context<Shell>) {
        let panes: HashSet<u64> = workspace.chat_panes().map(|(pane, _)| pane).collect();
        self.views.retain(|pane, _| panes.contains(pane));
        let shown: HashSet<ThreadId> = (workspace.chat_panes())
            .filter_map(|(_, thread)| thread.cloned())
            .collect();
        if shown == self.shown {
            return;
        }
        self.chats.update(cx, |chats, cx| {
            for gone in self.shown.difference(&shown) {
                chats.tab_closed(gone, cx);
            }
            for new in shown.difference(&self.shown) {
                chats.set_tab_open(new, true, cx);
            }
        });
        self.shown = shown;
    }
}

impl Shell {
    /// Makes the views the shown tab's chat panes don't have yet, so the panes draw them; the
    /// first pane's.
    pub fn make_shown_chats(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatView>> {
        let unit = self.workspace.branch()?;
        let panes = unit.tabs.get(unit.active_tab)?.panes();
        let mut chats = panes.iter().filter(|pane| pane.kind == PaneKind::Chat).peekable();
        let first = chats.peek()?.id;
        let missing: Vec<(u64, Option<ThreadId>)> = chats
            .filter(|pane| !self.chat_tabs.views.contains_key(&pane.id))
            .map(|pane| (pane.id, pane.chat.clone()))
            .collect();
        if !missing.is_empty() {
            let cwd = unit.worktree.path.clone();
            for (pane, thread) in missing {
                self.make_chat_view(pane, thread, cwd.clone(), window, cx);
            }
        }
        self.chat_tabs.view(first).cloned()
    }

    fn make_chat_view(
        &mut self,
        pane: u64,
        thread: Option<ThreadId>,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chats = self.chat_tabs.chats.clone();
        let view = cx.new(|cx| ChatView::new(chats, cwd, thread, window, cx));
        let events = cx.subscribe(&view, move |this, _, event, cx| match event {
            ChatViewEvent::Created(id) => this.chat_created(pane, id.clone(), cx),
        });
        if self.chat_tabs.focus_next.take() == Some(pane) {
            let focus = view.focus_handle(cx);
            cx.defer_in(window, move |_, window, cx| window.focus(&focus, cx));
        }
        self.chat_tabs.views.insert(
            pane,
            Made {
                view,
                _events: events,
            },
        );
    }

    /// The view of a chat pane that just opened takes the keyboard once it's made.
    pub(crate) fn focus_chat_next(&mut self) {
        let pane = (self.workspace.branch())
            .and_then(|unit| unit.tabs.get(unit.active_tab))
            .map(|tab| tab.first().id);
        self.chat_tabs.focus_next = pane;
    }

    /// The first send of pane `pane` made its thread.
    fn chat_created(&mut self, pane: u64, id: ThreadId, cx: &mut Context<Self>) {
        self.workspace.set_chat(pane, id);
        self.follow_chats(cx);
        cx.notify();
    }

    /// The tabs follow the summaries, and what's listed: the chats that had tabs when the app
    /// last ran get them, and a tab's title is its chat's, once it has one.
    pub(crate) fn follow_chats(&mut self, cx: &mut Context<Self>) {
        self.restore_chats(cx);
        let chats = self.chat_tabs.chats.read(cx);
        let changed = self.workspace.retitle_chats(|id| {
            let title = chats.summary(id).map(|summary| summary.title.as_str());
            title.filter(|title| !title.is_empty())
        });
        if changed {
            cx.notify();
        }
    }

    /// Brings back the chats that had tabs, each once its checkout is listed. One a pane shows
    /// already, made while the first list was on its way, has what it came for.
    fn restore_chats(&mut self, cx: &mut Context<Self>) {
        let chats = self.chat_tabs.chats.clone();
        let waiting: Vec<(ThreadId, PathBuf)> = {
            let chats = chats.read(cx);
            (chats.to_restore().iter())
                .filter_map(|id| Some((id.clone(), PathBuf::from(&chats.summary(id)?.cwd))))
                .collect()
        };
        for (id, cwd) in waiting {
            let shown = (self.workspace.chat_panes()).any(|(_, thread)| thread == Some(&id));
            if shown || self.workspace.add_chat_tab(&cwd, id.clone()) {
                chats.update(cx, |chats, _| chats.restored(&id));
                cx.notify();
            }
        }
    }

    fn close_chat_tabs(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        self.workspace.close_chat_panes(id);
        cx.notify();
    }
}
