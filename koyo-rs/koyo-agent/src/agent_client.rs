//! The app's one client of the agent host (`docs/chat.md` 5.1), as `TermClient` is of termd.
//! Nothing connects until the first request, and a request made after the connection ended
//! connects again, which starts a host that's gone. Whoever subscribed hears every event of
//! every connection, and word when one ends, ahead of anything from the next.

use std::future::Future;
use std::process::Command;
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;
use koyo_daemon::DaemonDir;
use parking_lot::Mutex;

use crate::client::{Client, ClientError, Connected, stop_host};
use crate::protocol::events::Event;
use crate::protocol::methods::{ClientKind, Method};
use crate::protocol::wire::HOST_VERSION;

/// What a subscriber hears.
#[derive(Clone, Debug)]
pub enum Notice {
    Event(Box<Event>),
    /// The connection ended: what it was watching must be asked for again. It comes before
    /// anything from the connection after it.
    Disconnected,
}

type Subscribers = Mutex<Vec<UnboundedSender<Notice>>>;

/// A request waiting its turn in the queue.
type Job = Box<dyn FnOnce() + Send>;

/// The connection in use, and the thread that passes its events on.
struct Live {
    client: Arc<Client>,
    pump: JoinHandle<()>,
}

pub struct AgentClient {
    dir: DaemonDir,
    command: Box<dyn Fn() -> Command + Send + Sync>,
    live: Mutex<Option<Live>>,
    subscribers: Arc<Subscribers>,
    queue: Sender<Job>,
}

impl AgentClient {
    /// A client of the host in `dir`, started with `command` when it isn't running. Nothing
    /// connects until the first request.
    pub fn new(
        dir: DaemonDir,
        command: impl Fn() -> Command + Send + Sync + 'static,
    ) -> std::io::Result<Arc<AgentClient>> {
        let (queue, jobs) = crossbeam_channel::unbounded::<Job>();
        std::thread::Builder::new()
            .name("agent-client-queue".into())
            .spawn(move || {
                for job in jobs {
                    job();
                }
            })?;
        Ok(Arc::new(AgentClient {
            dir,
            command: Box::new(command),
            live: Mutex::new(None),
            subscribers: Arc::default(),
            queue,
        }))
    }

    /// Every event from now on, and word when a connection ends.
    pub fn subscribe(&self) -> UnboundedReceiver<Notice> {
        let (tx, rx) = unbounded();
        self.subscribers.lock().push(tx);
        rx
    }

    /// Asks the host for `M` and waits for the answer, connecting first if need be; blocks, so
    /// call it off the UI thread. For what takes as long as it takes and depends on nothing
    /// else; what changes a thread goes through [`Self::ordered`].
    pub fn request<M: Method>(&self, params: &M::Params) -> Result<M::Result, ClientError> {
        self.connection()?.request::<M>(params)
    }

    /// Asks the host for `M` after everything asked this way before, whatever the caller does
    /// with the future: the request has its place in the queue as this returns. Doesn't block.
    pub fn ordered<M>(
        self: &Arc<Self>,
        params: M::Params,
    ) -> impl Future<Output = Result<M::Result, ClientError>> + Send + 'static + use<M>
    where
        M: Method + 'static,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
    {
        let (tx, answer) = oneshot::channel();
        let client = Arc::clone(self);
        // Only fails with the queue's thread gone, which drops `tx`, so the future says Closed.
        let _ = self.queue.send(Box::new(move || {
            let _ = tx.send(client.request::<M>(&params));
        }));
        async move { answer.await.unwrap_or(Err(ClientError::Closed)) }
    }

    /// Stops a host of another build, once the person chose to: the next request starts this
    /// build's in its place. Its running turns stop (`docs/chat.md` C9). Blocks like `request`.
    pub fn replace_host(&self) -> Result<(), ClientError> {
        // Held so that nothing connects between the stop and what the caller does next.
        let _live = self.live.lock();
        stop_host(&self.dir)
    }

    /// The connection in use, made first if it isn't open.
    fn connection(&self) -> Result<Arc<Client>, ClientError> {
        let mut live = self.live.lock();
        if let Some(open) = live.as_ref().filter(|live| !live.client.is_closed()) {
            return Ok(Arc::clone(&open.client));
        }
        // Its `Disconnected` goes out before anything from the next connection can.
        if let Some(ended) = live.take()
            && ended.pump.join().is_err()
        {
            log::error!("the agent client's event thread panicked");
        }
        let connected = self.connect()?;
        let client = Arc::new(connected.client);
        let (events, subscribers) = (connected.events, Arc::clone(&self.subscribers));
        let pump = std::thread::Builder::new()
            .name("agent-client-events".into())
            .spawn(move || pass_on(&events, &subscribers))?;
        *live = Some(Live {
            client: Arc::clone(&client),
            pump,
        });
        Ok(client)
    }

    /// Connects to a host of this build (`docs/chat.md` C9): one of another that's busy is left
    /// to the person, and one that's idle replaced without asking.
    fn connect(&self) -> Result<Connected, ClientError> {
        let start = || Client::connect_or_start(&self.dir, &self.command, ClientKind::App);
        let connected = start()?;
        if connected.hello.version == HOST_VERSION {
            return Ok(connected);
        }
        if connected.hello.busy {
            return Err(ClientError::OtherBuild);
        }
        log::info!(
            "replacing the idle agent host of version {}",
            connected.hello.version
        );
        drop(connected);
        stop_host(&self.dir)?;
        let connected = start()?;
        if connected.hello.version == HOST_VERSION {
            Ok(connected)
        } else {
            // Another build got there first; the two don't take turns replacing each other.
            Err(ClientError::OtherBuild)
        }
    }
}

/// Passes one connection's events on to the subscribers until it ends, then says so.
fn pass_on(events: &Receiver<Event>, subscribers: &Subscribers) {
    while let Ok(event) = events.recv() {
        tell(subscribers, &Notice::Event(Box::new(event)));
    }
    tell(subscribers, &Notice::Disconnected);
}

fn tell(subscribers: &Subscribers, notice: &Notice) {
    (subscribers.lock()).retain(|subscriber| subscriber.unbounded_send(notice.clone()).is_ok());
}
