//! Chat tabs on the shell (`docs/chat.md` 5.2). A chat pane's view is made when it's first shown.
//! Which chats have a tab follows from what the panes show, whichever way a tab came or went,
//! and the host is told, so the tabs come back with the app.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use futures::StreamExt as _;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, Entity, Focusable as _, InteractiveElement as _,
    SharedString, Subscription, Task, Window,
};
use koyo_agent::protocol::ids::ThreadId;

use super::view::details::{OpenIn, ToggleDetails, Workplace};
use super::view::{ChatView, ChatViewEvent};
use super::{Chats, ChatsEvent};
use crate::files::Place;
use crate::model::{PaneKind, Workspace};
use crate::native::notifications;
use crate::navigation::Route;
use crate::shell::Shell;

/// Shows the chat a person clicked a notification of, or only brings Koyo to the front for a
/// notice about anything else, for as long as the shell's there.
fn notifications_clicked(cx: &mut Context<Shell>) -> Option<Task<()>> {
    let mut clicks = notifications::clicks()?;
    Some(cx.spawn(async move |shell, cx| {
        while let Some(name) = clicks.next().await {
            let shown = shell.update(cx, |shell, cx| {
                if let Some(thread) = notifications::chat_of(&name) {
                    shell.open_chat(&ThreadId(thread.to_owned()), cx);
                }
                cx.activate(true);
            });
            if shown.is_err() {
                return;
            }
        }
    }))
}

/// How many chats with no tab the launcher is given to list.
const RECENT_CHATS: usize = 6;

pub struct ChatTabs {
    pub(super) chats: Entity<Chats>,
    /// Each chat pane's view once it's shown, by the pane's id.
    views: HashMap<u64, Made>,
    /// The threads the panes show.
    shown: HashSet<ThreadId>,
    /// The threads of panes the window brought back that the host hasn't yet said it still
    /// has (`restore_chats`).
    unconfirmed: HashSet<ThreadId>,
    /// What a pane's chat is told as soon as its view is made: the launcher's message.
    say_next: Option<(u64, String)>,
    _followed: Subscription,
    _clicked: Option<Task<()>>,
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
            ChatsEvent::News { id, title, said } => {
                notifications::post(&notifications::chat(&id.0), title, said);
            }
        });
        // A click on what a chat told a person shows that chat.
        let _clicked = notifications_clicked(cx);
        ChatTabs {
            chats,
            views: HashMap::new(),
            shown: HashSet::new(),
            unconfirmed: HashSet::new(),
            say_next: None,
            _followed,
            _clicked,
        }
    }

    /// The view of chat pane `pane`, once it's made.
    pub fn view(&self, pane: u64) -> Option<&Entity<ChatView>> {
        self.views.get(&pane).map(|made| &made.view)
    }

    /// A pane the window brought back shows thread `id` (`shell/kept.rs`): it keeps it if the
    /// host still has the chat, which its list says.
    pub(crate) fn bring_back(&mut self, id: &ThreadId) {
        self.unconfirmed.insert(id.clone());
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

/// Has the window's root hear what a chat hears on its own root, its card's key and its
/// menu, while the keyboard isn't in the chat: at the window, as after a press on what was
/// said or another space shown. They go on to the chat the person is in.
pub fn heard_at_window(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|shell, action: &ToggleDetails, window, cx| {
        shell.tell_chat(action, window, cx);
    }))
    .on_action(cx.listener(|shell, action: &OpenIn, window, cx| {
        shell.tell_chat(action, window, cx);
    }))
}

impl Shell {
    /// Makes the views the shown tab's chat panes don't have yet, so the panes draw them.
    pub fn make_shown_chats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((unit, tab)) = (self.workspace.branch()).zip(self.workspace.tab_shown()) else {
            return;
        };
        let mut chats = (tab.panes().into_iter())
            .filter(|pane| pane.kind == PaneKind::Chat)
            .peekable();
        if chats.peek().is_none() {
            return;
        }
        let missing: Vec<(u64, Option<ThreadId>)> = chats
            .filter(|pane| !self.chat_tabs.views.contains_key(&pane.id))
            .map(|pane| (pane.id, pane.chat.clone()))
            .collect();
        if !missing.is_empty() {
            let cwd = unit.worktree.path.clone();
            let project = self.workspace.project().map(|project| project.name.clone());
            let place: SharedString = match project {
                Some(project) => format!("{project} / {}", unit.name()).into(),
                None => unit.name(),
            };
            for (pane, thread) in missing {
                self.make_chat_view(pane, thread, cwd.clone(), place.clone(), window, cx);
            }
        }
        self.tell_chats_where_they_work(cx);
    }

    /// Tells the shown tab's chats where they work, once the right panel holds its checkout.
    fn tell_chats_where_they_work(&self, cx: &mut Context<Self>) {
        let (Some(project), Some(unit)) = (self.workspace.project(), self.workspace.branch())
        else {
            return;
        };
        let held = self
            .right_panel
            .checkout()
            .filter(|checkout| *checkout.read(cx).root() == unit.worktree.path);
        let (Some(checkout), Some(tab)) = (held, unit.tabs.get(unit.active_tab)) else {
            return;
        };
        for pane in tab.panes() {
            if let Some(view) = self.chat_tabs.view(pane.id) {
                let place = Workplace {
                    branch: unit.name(),
                    folder: unit.worktree.path.clone(),
                    project: project.path.clone(),
                    is_main: unit.is_main(),
                    checkout: checkout.clone(),
                };
                view.update(cx, |view, cx| view.set_place(place, cx));
            }
        }
    }

    fn make_chat_view(
        &mut self,
        pane: u64,
        thread: Option<ThreadId>,
        cwd: PathBuf,
        place: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chats = self.chat_tabs.chats.clone();
        let view = cx.new(|cx| ChatView::new(chats, cwd, place, thread, window, cx));
        let events = cx.subscribe_in(
            &view,
            window,
            move |this, _, event, window, cx| match event {
                ChatViewEvent::Created(id) => this.chat_created(pane, id.clone(), cx),
                ChatViewEvent::OpenFile(path) => {
                    this.open_file(Place::file(path.clone()), true, true, window, cx);
                }
                ChatViewEvent::RunScript { script, folder } => {
                    this.run_script(folder.clone(), script, cx);
                }
            },
        );
        if let Some((_, text)) = self.chat_tabs.say_next.take_if(|(next, _)| *next == pane) {
            let composer = view.read(cx).composer.clone();
            cx.defer_in(window, move |_, window, cx| {
                composer.update(cx, |composer, cx| composer.say(&text, window, cx));
            });
        }
        self.chat_tabs.views.insert(
            pane,
            Made {
                view,
                _events: events,
            },
        );
    }

    /// Hands `action`, heard at the window, on to the chat the person is in, where the
    /// workspace shows it.
    fn tell_chat(&mut self, action: &dyn Action, window: &mut Window, cx: &mut Context<Self>) {
        let chat = (self.route(cx) == Route::Workspace)
            .then(|| self.focused_pane(window, cx))
            .flatten()
            .and_then(|pane| self.chat_tabs.view(pane))
            .map(|chat| chat.focus_handle(cx));
        match chat {
            // With the keyboard in the chat it heard this first, and let it by.
            Some(chat) if !chat.contains_focused(window, cx) => {
                chat.dispatch_action(action, window, cx);
            }
            _ => cx.propagate(),
        }
    }

    /// Opens a new chat on the selected checkout and says `text` to it, as its composer would.
    pub(crate) fn ask_new_chat(&mut self, text: String, cx: &mut Context<Self>) {
        self.new_tab(PaneKind::Chat, cx);
        self.chat_tabs.say_next = self.shown_tab_key().map(|pane| (pane, text));
    }

    /// Shows thread `id`: the tab showing it already, or a new one on its checkout. The pane that
    /// shows it; `None` if there's no such chat, or its checkout isn't listed.
    pub(crate) fn open_chat(&mut self, id: &ThreadId, cx: &mut Context<Self>) -> Option<u64> {
        if let Some((at, pane)) = self
            .workspace
            .find_pane(|pane| pane.chat.as_ref() == Some(id))
        {
            self.pick_tab(at.0, at.1, at.2, cx);
            return Some(pane);
        }
        let cwd = PathBuf::from(&self.chat_tabs.chats.read(cx).summary(id)?.cwd);
        self.add_chat(&cwd, id.clone(), true, cx)
    }

    /// Thread `id` in a new tab on the checkout at `cwd`, shown with the keyboard if `select`. The
    /// pane that shows it; `None` if its checkout isn't listed.
    pub(crate) fn add_chat(
        &mut self,
        cwd: &Path,
        id: ThreadId,
        select: bool,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        // Moved to, not opened to type in: the keyboard stays where it is (`shell/keys.rs`).
        let pane = self.workspace.add_chat_tab(cwd, id, select)?;
        self.follow_chats(cx);
        Some(pane)
    }

    /// The chats, as app control asks after them.
    pub(crate) fn chats(&self) -> &Entity<Chats> {
        &self.chat_tabs.chats
    }

    /// The chats of the checkout at `worktree` no tab shows, latest first, with their titles.
    pub(crate) fn chats_without_tabs(
        &self,
        worktree: &Path,
        cx: &App,
    ) -> Vec<(ThreadId, SharedString)> {
        let tabs = &self.chat_tabs;
        let mut recent = tabs.chats.read(cx).latest_first(Some(worktree));
        recent.retain(|summary| !tabs.shown.contains(&summary.id));
        recent.truncate(RECENT_CHATS);
        let titled = |summary: &koyo_agent::protocol::model::ThreadSummary| -> SharedString {
            let title = summary.title.trim();
            if title.is_empty() {
                PaneKind::Chat.new_tab_title().into()
            } else {
                title.to_owned().into()
            }
        };
        recent
            .into_iter()
            .map(|summary| (summary.id.clone(), titled(summary)))
            .collect()
    }

    /// The first send of pane `pane` made its thread.
    fn chat_created(&mut self, pane: u64, id: ThreadId, cx: &mut Context<Self>) {
        self.workspace.set_chat(pane, id);
        self.follow_chats(cx);
    }

    /// The tabs follow the summaries, and what's listed: the chats that had tabs when the app
    /// last ran get them, and a tab's title is its chat's, once it has one.
    pub(crate) fn follow_chats(&mut self, cx: &mut Context<Self>) {
        self.restore_chats(cx);
        let chats = self.chat_tabs.chats.read(cx);
        self.workspace.retitle_chats(|id| {
            let title = chats.summary(id).map(|summary| summary.title.as_str());
            title.filter(|title| !title.is_empty())
        });
        // Whatever changed, the tabs and rows may say something else of them now, and what the
        // panes show may have changed.
        cx.notify();
    }

    /// Brings back the chats that had tabs, each once its checkout is listed. One a pane shows
    /// already, brought back with the window where it was or made while the first list was on
    /// its way, has what it came for; and a pane the window brought back for a chat the host
    /// no longer has, deleted while the window was closed, goes.
    fn restore_chats(&mut self, cx: &mut Context<Self>) {
        let chats = self.chat_tabs.chats.clone();
        let told: Vec<(ThreadId, bool)> = {
            let chats = chats.read(cx);
            (self.chat_tabs.unconfirmed.iter())
                .filter_map(|id| Some((id.clone(), chats.knows(id)?)))
                .collect()
        };
        for (id, kept) in told {
            self.chat_tabs.unconfirmed.remove(&id);
            if !kept {
                self.workspace.close_chat_panes(&id);
            }
        }
        let waiting: Vec<(ThreadId, PathBuf)> = {
            let chats = chats.read(cx);
            (chats.to_restore().iter())
                .filter_map(|id| Some((id.clone(), PathBuf::from(&chats.summary(id)?.cwd))))
                .collect()
        };
        for (id, cwd) in waiting {
            let shown = (self.workspace.chat_panes()).any(|(_, thread)| thread == Some(&id));
            if shown
                || self
                    .workspace
                    .add_chat_tab(&cwd, id.clone(), false)
                    .is_some()
            {
                chats.update(cx, |chats, _| chats.restored(&id));
            }
        }
    }

    fn close_chat_tabs(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        self.workspace.close_chat_panes(id);
        cx.notify();
    }
}
