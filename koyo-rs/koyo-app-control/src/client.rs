//! A blocking client of the control socket, for the stdio relay and the CLI: one connection,
//! many calls at once. Each call registers its `id`, writes its frame and waits for the
//! answer with that `id`; a reader thread of the connection's own hands each answer to its
//! caller in whatever order they come, and the events of followed topics to
//! [`Client::notices`]. When the connection ends, every waiting call and later call fails
//! with how it ended, and the notices say [`Notice::Lost`]; a relay then connects again.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use parking_lot::Mutex;
use serde_json::Value;

use crate::frame::{
    Call, ClientFrame, FrameError, Hello, MAX_CLIENT_FRAME, MAX_SERVER_FRAME, ServerFrame,
};
use crate::transport::{Connection, FrameReader, FrameWriter, nobody_listening};
use crate::{
    CONFIRM_TIMEOUT, Command, ControlError, Endpoint, Registered, Scope, Topic, check_request_id,
    info,
};

/// How long the app may take to welcome a client: it may be asking the person to allow it,
/// which waits two minutes.
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(130);
/// How long a call waits for its answer. A call the person may have to confirm waits
/// [`CONFIRM_TIMEOUT`] longer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(100);
/// How long a write waits for an app that doesn't read.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the client tells its owner besides answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A followed topic changed: read it again.
    Changed(Topic),
    /// The connection ended, as the error says; nothing more arrives on it.
    Lost(ControlError),
}

/// One connection to the app, welcomed with a grant. Calls may come from many threads at
/// once.
pub struct Client {
    writer: FrameWriter,
    shared: Arc<Shared>,
    grant: Vec<Scope>,
    next_id: AtomicU64,
    notices: Receiver<Notice>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

/// What the caller threads and the reader thread share.
struct Shared {
    waiting: Mutex<Waiting>,
    notices: Sender<Notice>,
}

#[derive(Default)]
struct Waiting {
    /// The calls waiting for their answers, by `id`.
    calls: HashMap<u64, Sender<Result<Value, ControlError>>>,
    /// How the connection ended, once it has.
    ended: Option<ControlError>,
}

impl Client {
    /// Connects to the app on `endpoint` and says `hello`.
    ///
    /// # Errors
    /// `app_not_running` when nothing listens; the app's refusal of the hello
    /// (`control_disabled`, `user_declined`, `invalid_input`); `timeout` when it doesn't
    /// answer within [`HELLO_TIMEOUT`].
    pub fn connect(endpoint: &Endpoint, hello: &Hello) -> Result<Client, ControlError> {
        let connection = Connection::connect(endpoint).map_err(|error| {
            if nobody_listening(&error) {
                ControlError::app_not_running()
            } else {
                ControlError::internal(format!(
                    "Can't reach Koyo at {}: {error}",
                    endpoint.path().display()
                ))
                .retryable(true)
            }
        })?;
        let (reader, writer) = connection.split().map_err(hung_up)?;
        writer.set_timeout(Some(WRITE_TIMEOUT)).map_err(hung_up)?;
        let (notify, notices) = crossbeam_channel::unbounded();
        let shared = Arc::new(Shared {
            waiting: Mutex::new(Waiting::default()),
            notices: notify,
        });
        // The reader thread reads the welcome too, so the hello's deadline is a wait on a
        // channel rather than a socket timeout, which macOS won't change once the app hangs up.
        let (welcomed, welcome) = crossbeam_channel::bounded(1);
        let reading = Arc::clone(&shared);
        let reader = std::thread::Builder::new()
            .name("app-control-client".into())
            .spawn(move || reading.run(reader, &welcomed))
            .map_err(|error| {
                ControlError::internal(format!("Can't start the client's reader: {error}"))
            })?;
        let hello = ClientFrame::Hello(hello.clone());
        let grant = writer
            .write(&hello, MAX_CLIENT_FRAME)
            .map_err(|error| match error {
                FrameError::Io(error) => hung_up(error),
                error => error.into(),
            })
            .and_then(|()| match welcome.recv_timeout(HELLO_TIMEOUT) {
                Ok(welcome) => welcome,
                Err(RecvTimeoutError::Timeout) => Err(ControlError::timeout(format!(
                    "Koyo didn't answer the hello within {} seconds.",
                    HELLO_TIMEOUT.as_secs()
                ))),
                Err(RecvTimeoutError::Disconnected) => Err(hung_up("its reader stopped")),
            });
        let grant = match grant {
            Ok(grant) => grant,
            Err(error) => {
                writer.shutdown();
                let _ = reader.join();
                return Err(error);
            }
        };
        Ok(Client {
            writer,
            shared,
            grant,
            next_id: AtomicU64::new(1),
            notices,
            reader: Mutex::new(Some(reader)),
        })
    }

    /// The scopes the app granted this client.
    pub fn grant(&self) -> &[Scope] {
        &self.grant
    }

    /// Whether the app granted `scope`.
    pub fn allows(&self, scope: Scope) -> bool {
        self.grant.contains(&scope)
    }

    /// Calls the operation named `operation` with `input`, waiting [`CALL_TIMEOUT`] for the
    /// answer (longer for one the person may confirm), and returns its output.
    ///
    /// # Errors
    /// The app's error; `invalid_input` for a malformed `request_id`; `timeout`; how the
    /// connection ended (`app_not_running`, retryable) when it ends first.
    pub fn call(
        &self,
        operation: &str,
        input: Value,
        request_id: Option<&str>,
    ) -> Result<Value, ControlError> {
        let timeout = info(operation)
            .filter(crate::Info::may_confirm)
            .map_or(CALL_TIMEOUT, |_| CALL_TIMEOUT + CONFIRM_TIMEOUT);
        self.call_within(operation, input, request_id, timeout)
    }

    /// [`Client::call`], waiting at most `timeout` for the answer.
    ///
    /// # Errors
    /// As [`Client::call`].
    pub fn call_within(
        &self,
        operation: &str,
        input: Value,
        request_id: Option<&str>,
        timeout: Duration,
    ) -> Result<Value, ControlError> {
        check_request_id(request_id)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (deliver, answer) = crossbeam_channel::bounded(1);
        {
            let mut waiting = self.shared.waiting.lock();
            if let Some(ended) = &waiting.ended {
                return Err(ended.clone());
            }
            waiting.calls.insert(id, deliver);
        }
        let frame = ClientFrame::Call(Call {
            id,
            request_id: request_id.map(str::to_owned),
            operation: operation.to_owned(),
            input,
        });
        if let Err(error) = self.writer.write(&frame, MAX_CLIENT_FRAME) {
            self.shared.waiting.lock().calls.remove(&id);
            return Err(error.into());
        }
        match answer.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.shared.waiting.lock().calls.remove(&id);
                // The answer may have come between the timeout and forgetting the call.
                answer.try_recv().unwrap_or_else(|_| {
                    Err(ControlError::timeout(format!(
                        "Koyo didn't answer {operation} within {} seconds.",
                        timeout.as_secs()
                    )))
                })
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(self.closed().unwrap_or_else(ControlError::connection_lost))
            }
        }
    }

    /// Calls `command`, as [`Client::call`].
    ///
    /// # Errors
    /// As [`Client::call`].
    pub fn call_command(
        &self,
        command: &Command,
        request_id: Option<&str>,
    ) -> Result<Value, ControlError> {
        let (operation, input) = command.to_parts();
        self.call(operation, input, request_id)
    }

    /// Calls `Op` with its typed input and reads its typed output.
    ///
    /// # Errors
    /// As [`Client::call`], or `internal` when the output isn't `Op`'s.
    pub fn call_op<Op: Registered>(
        &self,
        input: Op::Input,
        request_id: Option<&str>,
    ) -> Result<Op::Output, ControlError> {
        let output = self.call_command(&Op::command(input), request_id)?;
        serde_json::from_value(output).map_err(|error| {
            ControlError::internal(format!(
                "Koyo answered {} unexpectedly: {error}",
                Op::NAME
            ))
        })
    }

    /// Follows `topics` as well, until the connection ends; their events arrive as
    /// [`Notice::Changed`].
    ///
    /// # Errors
    /// How the connection ended, or a failed write.
    pub fn subscribe(&self, topics: &[Topic]) -> Result<(), ControlError> {
        if let Some(ended) = self.closed() {
            return Err(ended);
        }
        let frame = ClientFrame::Subscribe {
            topics: topics.to_vec(),
        };
        self.writer
            .write(&frame, MAX_CLIENT_FRAME)
            .map_err(ControlError::from)
    }

    /// The events of followed topics, and the connection's end. Every clone of the
    /// receiver takes from the same queue.
    pub fn notices(&self) -> Receiver<Notice> {
        self.notices.clone()
    }

    /// How the connection ended, once it has.
    pub fn closed(&self) -> Option<ControlError> {
        self.shared.waiting.lock().ended.clone()
    }

    /// Whether the connection ended, so calls on this client can only fail.
    pub fn is_closed(&self) -> bool {
        self.shared.waiting.lock().ended.is_some()
    }

    /// Hangs up. Waiting calls fail as if the app had closed the connection.
    pub fn close(&self) {
        self.writer.shutdown();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.writer.shutdown();
        if let Some(reader) = self.reader.lock().take() {
            let _ = reader.join();
        }
    }
}

impl Shared {
    /// Reads the app's answer to the hello, hands it to `welcomed`, then, once welcomed,
    /// reads the connection until it ends.
    fn run(&self, mut reader: FrameReader, welcomed: &Sender<Result<Vec<Scope>, ControlError>>) {
        let welcome = match reader.read::<ServerFrame>(MAX_SERVER_FRAME) {
            Ok(Some(ServerFrame::Welcome { grant })) => Ok(grant),
            Ok(Some(ServerFrame::Error { error, .. })) => Err(error),
            Ok(Some(_)) => Err(ControlError::internal(
                "Koyo answered the hello unexpectedly.",
            )),
            Ok(None) => Err(hung_up("it hung up")),
            Err(FrameError::Io(error)) => Err(hung_up(error)),
            Err(error) => Err(error.into()),
        };
        let welcomed_ok = welcome.is_ok();
        let _ = welcomed.send(welcome);
        if welcomed_ok {
            self.read_all(&mut reader);
        }
    }

    /// Reads frames until the connection ends, handing each answer to its call.
    fn read_all(&self, reader: &mut FrameReader) {
        let ended = loop {
            match reader.read::<ServerFrame>(MAX_SERVER_FRAME) {
                Ok(Some(ServerFrame::Result { id, output })) => self.deliver(id, Ok(output)),
                Ok(Some(ServerFrame::Error {
                    id: Some(id),
                    error,
                })) => self.deliver(id, Err(error)),
                Ok(Some(ServerFrame::Error { id: None, error })) => break error,
                Ok(Some(ServerFrame::Event { topic })) => {
                    let _ = self.notices.send(Notice::Changed(topic));
                }
                Ok(Some(ServerFrame::Welcome { .. })) => {
                    break ControlError::internal("Koyo sent a second welcome.");
                }
                Ok(None) | Err(FrameError::Io(_)) => break ControlError::connection_lost(),
                Err(error) => {
                    break ControlError::internal(format!(
                        "Koyo sent a frame this client can't read: {}",
                        ControlError::from(error).message
                    ));
                }
            }
        };
        self.end(&ended);
    }

    fn deliver(&self, id: u64, result: Result<Value, ControlError>) {
        if let Some(deliver) = self.waiting.lock().calls.remove(&id) {
            let _ = deliver.send(result);
        }
    }

    /// Fails every waiting call with `ended`, and every later one.
    fn end(&self, ended: &ControlError) {
        let calls = {
            let mut waiting = self.waiting.lock();
            waiting.ended = Some(ended.clone());
            std::mem::take(&mut waiting.calls)
        };
        for (_, deliver) in calls {
            let _ = deliver.send(Err(ended.clone()));
        }
        let _ = self.notices.send(Notice::Lost(ended.clone()));
    }
}

/// The app hung up before welcoming the client: it quit, or refused without saying why.
fn hung_up(why: impl std::fmt::Display) -> ControlError {
    ControlError::internal(format!(
        "Koyo closed the control connection before welcoming this client: {why}."
    ))
    .retryable(true)
}
