//! The chats (`docs/chat.md` 5.2). The agent host keeps them; the app knows what it was told:
//! every thread's summary, and the rows of the ones a tab shows. One loop takes in what the host
//! says, once a frame however fast it comes, and connects again when the connection ends.

mod asks;
mod attachments;
mod catalog;
mod composer;
mod drafts;
mod host;
mod marks;
mod queued;
mod output;
mod rows;
mod subagent_bar;
mod subagents;
mod tabs;
mod thread;
mod timeline;
mod view;
mod work;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, SharedString, Task, WeakEntity};
use koyo_agent::agent_client::{AgentClient, Notice};
use koyo_agent::client::{self, ClientError};
use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::ids::{RequestId, RunId, ThreadId};
use koyo_agent::protocol::methods::{
    Catalog, Delivery, Empty, HistoryParams, Method, OpenParams, RequestsRespond, RespondParams,
    Rows, SendParams, Sent, StopParams, ThreadParams, ThreadsClose, ThreadsHistory, ThreadsOpen,
    ThreadsUpdate, ThreadsWatch, TurnsSend, TurnsStop, UpdateParams,
};
use koyo_agent::protocol::model::{Prompt, ThreadSummary};
use koyo_agent::protocol::requests::Response;

use crate::settings;
use host::agent_client;
use thread::{ChatThread, ThreadEvent};

pub use tabs::ChatTabs;

/// How long the loop waits between batches: what arrives meanwhile is applied together.
const FRAME: Duration = Duration::from_millis(16);
/// How long after a connection ends it's made again, and the first wait after that fails.
const FIRST_WAIT: Duration = Duration::from_secs(1);
/// The longest wait between attempts to reach the host.
const LONGEST_WAIT: Duration = Duration::from_secs(30);
/// How many older runs come at a time.
const HISTORY_PAGE: usize = 30;

/// How things stand with the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    /// Nothing asked for it yet: a person who never opens a chat runs no host.
    Idle,
    Connecting,
    Connected,
    /// A host of another build runs chats; only the person can choose to stop them (C9).
    OtherBuild,
    /// It can't be reached, and why; it's tried again unless nothing can be done from here.
    Failed(SharedString),
}

/// What the shell follows of the chats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatsEvent {
    /// What a list of threads shows changed: the tabs follow.
    Summaries,
    /// A thread was deleted: its tabs go.
    Removed(ThreadId),
}

pub struct Chats {
    client: Option<Arc<AgentClient>>,
    summaries: HashMap<ThreadId, ThreadSummary>,
    /// The threads open with the host, for as long as anything shows them.
    threads: HashMap<ThreadId, WeakEntity<ChatThread>>,
    /// What each checkout a tab shows can choose from.
    catalogs: HashMap<PathBuf, WeakEntity<Catalog>>,
    connection: Connection,
    /// The threads that had tabs when the app last ran and have none yet, from the host's first
    /// list in this run: `None` until it comes. Later lists and summaries bring none back, so a
    /// chat just made, whose summary may come before the answer that names it, isn't one.
    to_restore: Option<Vec<ThreadId>>,
    /// The attempt to reach the host under way, if any.
    connecting: Option<Task<()>>,
    /// The loop over what the host says.
    _following: Option<Task<()>>,
}

impl EventEmitter<ChatsEvent> for Chats {}

/// What's bound for the chats, and what's kept of them.
pub fn init(cx: &mut App) {
    asks::bind_keys(cx);
    composer::bind_keys(cx);
    drafts::init(cx);
    queued::bind_keys(cx);
    subagent_bar::bind_keys(cx);
    view::details::bind_keys(cx);
}

impl Chats {
    /// The chats of the host in the app's data folder. A person who has chats has their host
    /// started now, to bring their tabs back; anyone else's starts with their first chat tab.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let client = agent_client();
        let connection = match client {
            Some(_) => Connection::Idle,
            None => Connection::Failed("Koyo's agent host isn't installed.".into()),
        };
        let _following = client.as_ref().map(|client| follow(client, cx));
        let mut chats = Chats {
            client,
            summaries: HashMap::new(),
            threads: HashMap::new(),
            catalogs: HashMap::new(),
            connection,
            to_restore: None,
            connecting: None,
            _following,
        };
        if settings::data_dir().is_some_and(|data| client::has_store(&client::host_dir(&data))) {
            chats.connect(cx);
        }
        chats
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn summary(&self, id: &ThreadId) -> Option<&ThreadSummary> {
        self.summaries.get(id)
    }

    /// The threads to bring back as tabs, once the host has listed them.
    pub fn to_restore(&self) -> &[ThreadId] {
        self.to_restore.as_deref().unwrap_or_default()
    }

    /// Thread `id` has its tab back.
    pub fn restored(&mut self, id: &ThreadId) {
        if let Some(waiting) = &mut self.to_restore {
            waiting.retain(|other| other != id);
        }
    }

    /// Reaches the host unless it's reached, or being reached.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        if matches!(self.connection, Connection::Idle) {
            self.reach(Duration::ZERO, cx);
        }
    }

    /// Stops a host of another build and starts this one's, as the person chose: its running
    /// turns stop.
    pub fn replace_host(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.connection = Connection::Connecting;
        cx.notify();
        let stopping = cx.background_spawn(async move { client.replace_host() });
        self.connecting = Some(cx.spawn(async move |this, cx| {
            let stopped = stopping.await;
            this.update(cx, |this, cx| match stopped {
                Ok(()) => this.reach(Duration::ZERO, cx),
                Err(error) => this.failed(&error, cx),
            })
            .ok();
        }));
    }

    /// Asks the host for its threads after `wait`, over and over until it answers, then opens
    /// what was open before: the way back from any connection that ended.
    fn reach(&mut self, wait: Duration, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.connection = Connection::Connecting;
        cx.notify();
        self.connecting = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let mut wait = FIRST_WAIT;
            loop {
                let asking = Arc::clone(&client);
                let listed = cx
                    .background_spawn(async move { asking.request::<ThreadsWatch>(&Empty {}) })
                    .await;
                let again = this.update(cx, |this, cx| this.listed(listed, cx));
                if !again.unwrap_or(false) {
                    return;
                }
                cx.background_executor().timer(wait).await;
                wait = (wait * 2).min(LONGEST_WAIT);
            }
        }));
    }

    /// Takes in the host's list of threads; whether to ask again.
    fn listed(
        &mut self,
        listed: Result<Vec<ThreadSummary>, ClientError>,
        cx: &mut Context<Self>,
    ) -> bool {
        let summaries = match listed {
            Ok(summaries) => summaries,
            Err(ClientError::OtherBuild) => {
                self.connection = Connection::OtherBuild;
                cx.notify();
                return false;
            }
            Err(error) => {
                self.failed(&error, cx);
                return true;
            }
        };
        self.to_restore.get_or_insert_with(|| {
            let mut tabs: Vec<ThreadId> = (summaries.iter())
                .filter(|summary| summary.tab_open)
                .map(|summary| summary.id.clone())
                .collect();
            // In the order they were made, as the host's ids sort, so they come back the same.
            tabs.sort();
            tabs
        });
        self.summaries = (summaries.into_iter())
            .map(|summary| (summary.id.clone(), summary))
            .collect();
        self.connection = Connection::Connected;
        for thread in self.threads.values().filter_map(WeakEntity::upgrade) {
            self.open(thread, cx);
        }
        self.refresh_catalogs(cx);
        cx.emit(ChatsEvent::Summaries);
        cx.notify();
        false
    }

    fn failed(&mut self, error: &ClientError, cx: &mut Context<Self>) {
        log::warn!("can't reach the agent host: {error}");
        self.connection = Connection::Failed(error.to_string().into());
        cx.notify();
    }

    /// The open thread `id`, opened now if it isn't. It stays open as long as it's held.
    pub fn thread(&mut self, id: &ThreadId, cx: &mut Context<Self>) -> Entity<ChatThread> {
        if let Some(thread) = self.threads.get(id).and_then(WeakEntity::upgrade) {
            return thread;
        }
        let thread = cx.new(|_| ChatThread::new(id.clone()));
        let released = id.clone();
        cx.observe_release(&thread, move |chats, _, cx| chats.let_go(&released, cx)).detach();
        self.threads.insert(id.clone(), thread.downgrade());
        self.open(thread.clone(), cx);
        thread
    }

    /// Nothing holds thread `id` any more. The host hears so, unless it deleted the thread, and
    /// what's kept of it is the host's. It may be open again by now, which isn't this one.
    fn let_go(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        if self.threads.get(id).is_some_and(|thread| thread.upgrade().is_none()) {
            self.threads.remove(id);
            let params = ThreadParams {
                thread_id: id.clone(),
            };
            logged(self.ordered::<ThreadsClose>(params), "closing a chat", cx);
        }
    }

    /// Asks the host to open `thread`, from what it holds, its snapshot taken in once it comes.
    fn open(&self, thread: Entity<ChatThread>, cx: &mut Context<Self>) {
        let Some(client) = &self.client else {
            return;
        };
        let opened = thread.update(cx, |thread, _| {
            client.ordered::<ThreadsOpen>(OpenParams {
                thread_id: thread.id().clone(),
                since_seq: thread.opening(),
            })
        });
        cx.spawn(async move |_, cx| {
            let opened = opened.await;
            thread.update(cx, |thread, cx| thread.opened(opened, cx));
        })
        .detach();
    }

    /// No pane shows thread `id` any more. The host hears so, unless it deleted the thread.
    fn tab_closed(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        if self.threads.contains_key(id) || self.summaries.contains_key(id) {
            self.set_tab_open(id, false, cx);
        }
    }

    /// Tells the host whether a tab shows thread `id`, for the tabs to come back as they were.
    fn set_tab_open(&self, id: &ThreadId, open: bool, cx: &mut Context<Self>) {
        let params = UpdateParams {
            thread_id: id.clone(),
            title: None,
            model: None,
            effort: None,
            fast: None,
            permission: None,
            plan: None,
            tab_open: Some(open),
        };
        logged(self.ordered::<ThreadsUpdate>(params), "noting a chat's tab", cx);
    }

    /// Sends `prompt` to thread `id`, to go as `delivery` says.
    pub fn send(
        &self,
        id: &ThreadId,
        prompt: Prompt,
        delivery: Delivery,
    ) -> impl Future<Output = Result<Sent, ClientError>> + use<> {
        self.ordered::<TurnsSend>(SendParams {
            thread_id: id.clone(),
            prompt,
            delivery,
        })
    }

    /// Says `response` to request `request` of thread `id`.
    pub fn respond(
        &self,
        id: &ThreadId,
        request: RequestId,
        response: Response,
    ) -> impl Future<Output = Result<Empty, ClientError>> + use<> {
        self.ordered::<RequestsRespond>(RespondParams {
            thread_id: id.clone(),
            request_id: request,
            response,
        })
    }

    /// Asks what's running in thread `id` to stop.
    pub fn stop(&self, id: &ThreadId, cx: &mut Context<Self>) {
        let params = StopParams {
            thread_id: id.clone(),
            force: false,
        };
        logged(self.ordered::<TurnsStop>(params), "stopping a chat's run", cx);
    }

    /// A page of the runs of thread `id` sent before `before`, on its own thread as it may take a
    /// while, and with nothing to come in order with.
    pub fn history(&self, id: &ThreadId, before: RunId, cx: &App) -> Task<Result<Rows, ClientError>> {
        let Some(client) = self.client.clone() else {
            return Task::ready(Err(ClientError::Closed));
        };
        let params = HistoryParams {
            thread_id: id.clone(),
            before,
            runs: HISTORY_PAGE as u32,
        };
        cx.background_spawn(async move { client.request::<ThreadsHistory>(&params) })
    }

    /// Asks the host for `M` in its place in the queue (`AgentClient::ordered`).
    fn ordered<M>(
        &self,
        params: M::Params,
    ) -> impl Future<Output = Result<M::Result, ClientError>> + Send + 'static + use<M>
    where
        M: Method + 'static,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
    {
        let asked = (self.client.as_ref())
            .map(|client| client.ordered::<M>(params))
            .ok_or(ClientError::Closed);
        async move { asked?.await }
    }

    /// Applies one batch of what the host said, in order, each thread touched once.
    fn take_in(&mut self, notices: Vec<Notice>, cx: &mut Context<Self>) {
        let mut touched: HashMap<ThreadId, Vec<ThreadEvent>> = HashMap::new();
        let mut ended = false;
        let mut summaries = false;
        for notice in notices {
            let Notice::Event(event) = notice else {
                ended = true;
                continue;
            };
            match *event {
                Event::ThreadChanged(changed) => {
                    (touched.entry(changed.thread_id.clone()).or_default())
                        .push(ThreadEvent::Changed(Box::new(changed)));
                }
                Event::ItemAppended(appended) => {
                    (touched.entry(appended.thread_id.clone()).or_default())
                        .push(ThreadEvent::Appended(appended));
                }
                Event::ThreadSummary(summary) => {
                    self.summaries.insert(summary.id.clone(), summary);
                    summaries = true;
                }
                Event::ThreadRemoved(removed) => {
                    self.summaries.remove(&removed.thread_id);
                    self.threads.remove(&removed.thread_id);
                    drafts::thread_removed(&removed.thread_id, cx);
                    cx.emit(ChatsEvent::Removed(removed.thread_id));
                }
                Event::HostNotice(notice) => log::info!("the agent host: {}", notice.message),
                Event::Unknown { event, .. } => log::debug!("an event of a newer host: {event}"),
            }
        }
        for (id, events) in touched {
            if let Some(thread) = self.threads.get(&id).and_then(WeakEntity::upgrade) {
                thread.update(cx, |thread, cx| thread.take_in(events, cx));
            }
        }
        if summaries {
            cx.emit(ChatsEvent::Summaries);
        }
        if ended {
            self.reach(FIRST_WAIT, cx);
        }
    }
}

/// The loop over what the host says: it takes one notice, then all that wait, applies them
/// together and waits out the frame, so that nothing is applied more than once a frame.
fn follow(client: &AgentClient, cx: &mut Context<Chats>) -> Task<()> {
    let mut inbox = client.subscribe();
    cx.spawn(async move |this, cx| {
        while let Some(first) = inbox.next().await {
            let mut notices = vec![first];
            while let Ok(notice) = inbox.try_recv() {
                notices.push(notice);
            }
            if this.update(cx, |this, cx| this.take_in(notices, cx)).is_err() {
                return;
            }
            cx.background_executor().timer(FRAME).await;
        }
    })
}

/// Hears how `request`, in the queue already, went, with word in the log if it failed.
fn logged<T: Send + 'static>(
    request: impl Future<Output = Result<T, ClientError>> + Send + 'static,
    what: &'static str,
    cx: &mut Context<Chats>,
) {
    cx.background_spawn(async move {
        if let Err(error) = request.await {
            log::warn!("{what}: {error}");
        }
    })
    .detach();
}

#[cfg(test)]
mod tests;
