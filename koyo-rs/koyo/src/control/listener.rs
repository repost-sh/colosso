//! The control socket's server: a thread accepting connections, and per connection a thread
//! reading its frames and one writing the app's, so the UI thread never waits on a socket.
//! The reader checks each frame and hands hellos, calls and subscriptions to the UI thread on
//! the channel; answers and events come back through the writer's queue, in whatever order
//! they're ready.

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use koyo_app_control::frame::{FrameError, MAX_CLIENT_FRAME, MAX_SERVER_FRAME, decode_client};
use koyo_app_control::transport::{Connection, FrameReader, FrameWriter, Listener};
use koyo_app_control::{
    ClientFrame, Command, ControlError, Endpoint, HELLO_TIMEOUT, ServerFrame, check_request_id,
};
use futures::channel::mpsc::UnboundedSender;

use super::pipeline::Pending;
use super::{Caller, Request, Welcome, ask, next_connection, topics};

/// How long a new connection may take to say hello.
const HELLO_WAIT: Duration = Duration::from_secs(30);
/// How many failures to accept in a row make the server give up.
const ACCEPT_FAILURES: u32 = 100;

/// The socket's server, while the app runs. Dropping it stops accepting and removes the
/// socket file.
pub(crate) struct Server {
    listener: Arc<Listener>,
    endpoint: Endpoint,
    stop: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl Server {
    /// Serves `listener`, bound at `endpoint`, handing what clients ask to `requests`.
    pub fn start(
        listener: Listener,
        endpoint: Endpoint,
        requests: UnboundedSender<Request>,
    ) -> io::Result<Server> {
        let listener = Arc::new(listener);
        let stop = Arc::new(AtomicBool::new(false));
        let accepting = thread::Builder::new()
            .name("app-control-accept".into())
            .spawn({
                let (listener, stop) = (Arc::clone(&listener), Arc::clone(&stop));
                move || accept(&listener, &stop, &requests)
            })?;
        Ok(Server {
            listener,
            endpoint,
            stop,
            accepting: Some(accepting),
        })
    }

    /// The socket file.
    pub fn path(&self) -> &Path {
        self.listener.path()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // A connection wakes the accepting thread to see it should stop and let go of the
        // listener, whose file goes with the last of it.
        if Connection::connect(&self.endpoint).is_ok()
            && let Some(accepting) = self.accepting.take()
        {
            accepting.join().ok();
        }
    }
}

fn accept(listener: &Listener, stop: &AtomicBool, requests: &UnboundedSender<Request>) {
    let mut failures = 0;
    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match accepted {
            Ok(connection) => {
                failures = 0;
                let requests = requests.clone();
                let serving = thread::Builder::new()
                    .name("app-control-connection".into())
                    .spawn(move || serve(connection, &requests));
                if let Err(error) = serving {
                    log::error!("couldn't serve an app-control connection: {error}");
                }
            }
            Err(error) => {
                failures += 1;
                log::warn!("couldn't accept an app-control connection: {error}");
                if failures == ACCEPT_FAILURES {
                    log::error!(
                        "app control stops accepting connections after {failures} failures"
                    );
                    return;
                }
            }
        }
    }
}

/// What the writer is handed: a frame, or a last frame after which the connection closes.
enum Out {
    Frame(ServerFrame),
    Close(ServerFrame),
}

/// Serves one connection until it closes.
pub(crate) fn serve(connection: Connection, requests: &UnboundedSender<Request>) {
    let (mut reader, writer) = match connection.split() {
        Ok(halves) => halves,
        Err(error) => return log::warn!("couldn't use an app-control connection: {error}"),
    };
    let (out, outgoing) = mpsc::channel();
    let writing = thread::Builder::new()
        .name("app-control-writer".into())
        .spawn(move || write(&writer, &outgoing));
    if let Err(error) = writing {
        return log::error!("couldn't answer an app-control connection: {error}");
    }
    let Some(welcome) = welcome(&mut reader, &out, requests) else {
        return;
    };
    let connection = next_connection();
    read(connection, &welcome.caller, &mut reader, &out, requests);
    requests.unbounded_send(Request::Closed { connection }).ok();
}

fn write(writer: &FrameWriter, outgoing: &Receiver<Out>) {
    for out in outgoing {
        let (frame, last) = match out {
            Out::Frame(frame) => (frame, false),
            Out::Close(frame) => (frame, true),
        };
        if let Err(error) = writer.write(&frame, MAX_SERVER_FRAME) {
            log::debug!("an app-control client went: {error:?}");
            break;
        }
        if last {
            break;
        }
    }
    writer.shutdown();
}

/// Refuses the connection with `error`, which ends it.
fn refuse(out: &Sender<Out>, error: ControlError) {
    out.send(Out::Close(ServerFrame::refusal(error))).ok();
}

/// Reads the connection's hello and has the UI thread answer it: the welcome, written back,
/// or `None` once the connection is refused.
fn welcome(
    reader: &mut FrameReader,
    out: &Sender<Out>,
    requests: &UnboundedSender<Request>,
) -> Option<Welcome> {
    reader.set_timeout(Some(HELLO_WAIT)).ok();
    let line = match reader.read_line(MAX_CLIENT_FRAME) {
        Ok(Some(line)) => line,
        Ok(None) => return None,
        Err(error) => {
            refuse(out, error.into());
            return None;
        }
    };
    reader.set_timeout(None).ok();
    let hello = match decode_client(&line) {
        Ok(ClientFrame::Hello(hello)) => hello,
        Ok(_) => {
            refuse(out, ControlError::invalid_input("Say hello first."));
            return None;
        }
        Err((_, error)) => {
            refuse(out, error);
            return None;
        }
    };
    match ask(requests, HELLO_TIMEOUT, |reply| Request::Hello {
        hello,
        reply,
    }) {
        Ok(welcome) => {
            let grant = welcome.grant.clone();
            out.send(Out::Frame(ServerFrame::Welcome { grant })).ok()?;
            Some(welcome)
        }
        Err(error) => {
            refuse(out, error);
            None
        }
    }
}

/// Reads the welcomed connection's frames until it closes.
fn read(
    connection: u64,
    caller: &Caller,
    reader: &mut FrameReader,
    out: &Sender<Out>,
    requests: &UnboundedSender<Request>,
) {
    loop {
        let line = match reader.read_line(MAX_CLIENT_FRAME) {
            Ok(Some(line)) => line,
            Ok(None) | Err(FrameError::Io(_)) => return,
            Err(error) => return refuse(out, error.into()),
        };
        let request = match decode_client(&line) {
            Ok(ClientFrame::Call(call)) => {
                let id = call.id;
                let parsed = check_request_id(call.request_id.as_deref())
                    .and_then(|()| Command::parse(&call.operation, call.input));
                let command = match parsed {
                    Ok(command) => command,
                    Err(error) => {
                        out.send(Out::Frame(ServerFrame::answer(id, Err(error))))
                            .ok();
                        continue;
                    }
                };
                let answers = out.clone();
                Request::Call(Pending {
                    caller: caller.clone(),
                    request_id: call.request_id,
                    command,
                    reply: Box::new(move |result| {
                        answers
                            .send(Out::Frame(ServerFrame::answer(id, result)))
                            .ok();
                    }),
                })
            }
            Ok(ClientFrame::Subscribe { topics }) => {
                let (events, refusals) = (out.clone(), out.clone());
                let sink: topics::Sink = Box::new(move |topic| {
                    events
                        .send(Out::Frame(ServerFrame::Event { topic }))
                        .is_ok()
                });
                Request::Subscribe {
                    connection,
                    caller: caller.clone(),
                    topics,
                    sink,
                    reply: Box::new(move |result| {
                        if let Err(error) = result {
                            refuse(&refusals, error);
                        }
                    }),
                }
            }
            Ok(ClientFrame::Hello(_)) => {
                return refuse(
                    out,
                    ControlError::invalid_input("This connection said hello already."),
                );
            }
            Err((Some(id), error)) => {
                out.send(Out::Frame(ServerFrame::answer(id, Err(error))))
                    .ok();
                continue;
            }
            Err((None, error)) => return refuse(out, error),
        };
        if requests.unbounded_send(request).is_err() {
            return refuse(out, ControlError::app_not_running());
        }
    }
}
