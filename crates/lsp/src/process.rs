//! The server's process: started with its three pipes, each served by a thread of its own,
//! and a fourth thread that waits for it to end and says how.

use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use lsp_types::notification::Notification;

use crate::client::Launch;
use crate::error::LspError;
use crate::frame;
use crate::handlers::{self, Responder};
use crate::message::{Incoming, Payload, span};
use crate::shared::Shared;
use crate::status::ServerStatus;
use crate::subscription::publish;

/// How long a server's last stderr lines may take to arrive after it exits, for its report.
const STDERR_GRACE: Duration = Duration::from_millis(200);
/// How often a server that closed its output is checked for having exited. Only that short
/// window is checked this way; a running server is waited on without waking.
const EXIT_CHECK: Duration = Duration::from_millis(5);

/// What the process's watcher is told.
pub(crate) enum Signal {
    /// The server closed its output, so it's exiting.
    OutputClosed,
    /// Kill it: shutdown timed out, or the client is gone, or, with the reason, its output
    /// can't be read any more, so it is of no further use.
    Kill(Option<String>),
}

/// Starts the server and its threads. The returned sender tells its watcher to kill it.
pub(crate) fn start(launch: &Launch) -> Result<(Arc<Shared>, Sender<Signal>), LspError> {
    let spawn_error =
        |error: io::Error| LspError::Spawn(format!("{}: {error}", launch.program.display()));
    let mut child = Command::new(&launch.program)
        .args(&launch.args)
        .envs(launch.env.iter().map(|(key, value)| (key, value)))
        .current_dir(&launch.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(spawn_error)?;
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    let guarded = Watched(child);
    let (Some(stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) else {
        return Err(LspError::Spawn(
            "the server's pipes weren't opened".to_owned(),
        ));
    };

    let name = launch.name.clone();
    let (out, outgoing) = mpsc::channel();
    let shared = Arc::new(Shared::new(&name, out, launch.settings.clone()));
    let (signal, signals) = mpsc::channel();
    let (stderr_done, stderr_finished) = mpsc::channel();

    // The watcher first: it owns the process, and kills it if anything after this fails.
    let reporter = Arc::clone(&shared);
    spawn(&name, "watch", move || {
        watch(guarded, &signals, &stderr_finished, &reporter);
    })
    .map_err(spawn_error)?;
    let started = (|| {
        spawn(&name, "write", move || write(stdin, &outgoing))?;
        let errors = Arc::clone(&shared);
        spawn(&name, "stderr", move || {
            read_stderr(stderr, &errors, &stderr_done);
        })?;
        let reader = Arc::clone(&shared);
        let closed = signal.clone();
        spawn(&name, "read", move || read(stdout, &reader, &closed))
    })();
    if let Err(error) = started {
        let _ = signal.send(Signal::Kill(None));
        return Err(spawn_error(error));
    }
    Ok((shared, signal))
}

fn spawn(name: &str, job: &str, run: impl FnOnce() + Send + 'static) -> io::Result<()> {
    thread::Builder::new()
        .name(format!("lsp {name} {job}"))
        .spawn(run)
        .map(drop)
}

/// The process, killed if it's dropped before it's been waited for, so a server can't
/// outlive a client that failed to start.
struct Watched(Child);

impl Drop for Watched {
    fn drop(&mut self) {
        // Both are no-ops once the process has been waited for.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Waits for the process to end, kills it when told to, and reports how it ended.
fn watch(
    mut watched: Watched,
    signals: &Receiver<Signal>,
    stderr_finished: &Receiver<()>,
    shared: &Shared,
) {
    let (status, killed, broken) = wait(&mut watched.0, signals);
    // A crash report is about the last stderr lines; give their reader a moment to finish.
    let _ = stderr_finished.recv_timeout(STDERR_GRACE);
    let code = match status {
        Ok(status) => status.code(),
        Err(error) => {
            log::error!("couldn't wait for {}: {error}", shared.name);
            None
        }
    };
    shared.ended(code, killed, broken.as_deref());
}

/// Waits for the process to end, killing it when told to: how it ended, whether it was
/// killed, and why if it broke the protocol. Until the server closes its output nothing can
/// end it but a kill; after, it's checked for having exited while a kill is still taken, as
/// a process can close its output and keep running.
fn wait(
    child: &mut Child,
    signals: &Receiver<Signal>,
) -> (io::Result<ExitStatus>, bool, Option<String>) {
    let mut output_closed = false;
    loop {
        let signal = if output_closed {
            match child.try_wait() {
                Ok(Some(status)) => return (Ok(status), false, None),
                Ok(None) => {}
                Err(error) => return (Err(error), false, None),
            }
            match signals.recv_timeout(EXIT_CHECK) {
                Ok(signal) => Some(signal),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => None,
            }
        } else {
            signals.recv().ok()
        };
        let broken = match signal {
            Some(Signal::OutputClosed) => {
                output_closed = true;
                continue;
            }
            Some(Signal::Kill(broken)) => broken,
            // The client and the reader are both gone, so nothing else could tell.
            None => None,
        };
        let killed = kill(child);
        return (child.wait(), killed, broken);
    }
}

/// Kills the process; `true` if it was still running to be killed.
fn kill(child: &mut Child) -> bool {
    match child.try_wait() {
        Ok(Some(_)) => false,
        _ => child.kill().is_ok(),
    }
}

/// Writes each queued message, flushing once the queue is empty, until the queue closes or
/// the server stops reading.
fn write(stdin: ChildStdin, outgoing: &Receiver<Vec<u8>>) {
    let mut writer = BufWriter::with_capacity(64 * 1024, stdin);
    let mut written = || -> io::Result<()> {
        while let Ok(body) = outgoing.recv() {
            frame::write(&mut writer, &body)?;
            while let Ok(body) = outgoing.try_recv() {
                frame::write(&mut writer, &body)?;
            }
            writer.flush()?;
        }
        Ok(())
    };
    if let Err(error) = written() {
        log::debug!("stopped writing to the server: {error}");
    }
}

/// Reads messages and hands each to whoever waits for it, until the output closes.
fn read(stdout: ChildStdout, shared: &Arc<Shared>, signal: &Sender<Signal>) {
    let mut reader = BufReader::with_capacity(64 * 1024, stdout);
    let mut line = String::new();
    let ended = loop {
        match frame::read(&mut reader, &mut line) {
            Ok(Some(body)) => dispatch(shared, body),
            Ok(None) => break Signal::OutputClosed,
            // Past a broken frame there's no knowing where the next message starts.
            Err(error) => break Signal::Kill(Some(format!("unreadable output: {error}"))),
        }
    };
    let _ = signal.send(ended);
}

/// Finds out what a message is and who it's for, and hands it over unparsed.
fn dispatch(shared: &Arc<Shared>, body: Vec<u8>) {
    let message: Incoming<'_> = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(error) => {
            // One bad message isn't fatal: the frames around it are still whole.
            log::warn!("{} sent an unreadable message: {error}", shared.name);
            return;
        }
    };
    let Incoming {
        id,
        method,
        params,
        result,
        error,
    } = message;
    match (id, method) {
        (Some(id), Some(method)) => {
            log::trace!("{} asked {method}", shared.name);
            let responder = Responder::new(id, &method, shared.out.clone());
            let range = span(&body, params);
            handlers::answer(shared, &Payload::new(body, range), responder);
        }
        (None, Some(method)) => {
            if method == ServerStatus::METHOD {
                shared.status_changed(params);
            }
            let subscribers = shared.subscribers(&method);
            if subscribers.is_empty() {
                log::trace!("{} told {method}, with nobody listening", shared.name);
                return;
            }
            let range = span(&body, params);
            publish(&subscribers, Payload::new(body, range));
        }
        (Some(id), None) => {
            let Ok(number) = serde_json::from_str::<i32>(id.get()) else {
                log::warn!("{} answered a request we never made: {id}", shared.name);
                return;
            };
            let answer = if let Some(error) = error {
                Err(error.into())
            } else {
                let range = span(&body, result);
                Ok(Payload::new(body, range))
            };
            if !shared.answer(number, answer) {
                log::debug!("{} answered request {number}, given up on", shared.name);
            }
        }
        (None, None) => match error {
            Some(error) => log::warn!("{} reported an error: {error}", shared.name),
            None => log::warn!("{} sent a message that's neither", shared.name),
        },
    }
}

/// Logs the server's stderr line by line, keeping the last lines for a crash report.
fn read_stderr(stderr: ChildStderr, shared: &Shared, done: &Sender<()>) {
    for line in BufReader::new(stderr).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end();
        if !line.is_empty() {
            log::info!("{}: {line}", shared.name);
            shared.stderr_line(line.to_owned());
        }
    }
    let _ = done.send(());
}
