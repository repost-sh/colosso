//! A daemon of another build under the app (`docs/terminals.md` 5.1): a stand-in on the
//! daemon's socket that welcomes clients with another engine and stops when told to. The
//! person's shell comes back as a tab that says so; the notice's button stops the stand-in,
//! and the shell runs again on this build's daemon.

use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use colosso_term::TerminalView;
use colosso_term_control::{
    Actor, Decoder, ErrorCode, Message, Outcome, PROTOCOL, ProtocolError, Request, RequestBody,
    Response, ResponseBody, SNAPSHOT_FORMAT, Status, TermDir, Welcome,
};

use super::{control, stop_daemon, terminal, wait_until};
use crate::shell::Shell;
use crate::visual::{Drawn, Result};

pub(super) fn other_build(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    let shell = drawn.shell.clone();
    let closed = terminal(drawn)?.downgrade();
    (drawn.cx).update(|cx| shell.update(cx, Shell::close_shown_terminal));
    wait_until(drawn, "the tab to close", |drawn| {
        Ok(terminal(drawn).is_err())
    })?;
    wait_until(drawn, "its view to go", |_| Ok(closed.upgrade().is_none()))?;
    let client = drawn
        .cx
        .update(|cx| shell.update(cx, |shell, _| shell.terminal_client()));
    let dir = client
        .ok_or("the app has no terminal client")?
        .dir()
        .clone();
    // Held while the daemons change places, so the app can't start one in between.
    let lock = std::fs::File::create(dir.start_lock()).map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    stop_daemon(&dir)?;
    wait_until(drawn, "the daemon to stop", |_| Ok(!dir.socket().exists()))?;
    let older = Older::start(&dir)?;
    drop(lock);

    (drawn.cx).update(|cx| shell.update(cx, Shell::restore_terminals_again));
    wait_until(
        drawn,
        "a tab saying the terminals are another build's",
        |drawn| {
            Ok(
                terminal(drawn)
                    .is_ok_and(|view| drawn.cx.read(|cx| view.read(cx).is_other_build())),
            )
        },
    )?;
    said.push(format!("{}", drawn.save("other-build.png")?.display()));

    // As the notice's button does.
    let view = terminal(drawn)?;
    (drawn.cx).update(|cx| view.update(cx, TerminalView::replace_daemon));
    drop(view);
    wait_until(drawn, "the other build's daemon to stop", |_| {
        Ok(older.stopped())
    })?;
    control::holds(
        drawn,
        "the shell back on this build's daemon, at its prompt",
        |state| state.status == Status::Idle && state.reason == "prompt",
    )?;
    said.push(format!("{}", drawn.save("replaced.png")?.display()));
    said.push(
        "a daemon of another build was replaced from the tab's notice, and the shell came back"
            .into(),
    );
    Ok(())
}

/// A stand-in for another build's daemon, serving on its own thread.
struct Older {
    stopped: Arc<AtomicBool>,
}

impl Older {
    fn start(dir: &TermDir) -> Result<Older> {
        let socket = dir.socket();
        let listener = UnixListener::bind(&socket)
            .map_err(|error| format!("couldn't stand in for the daemon: {error}"))?;
        let stopped = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&stopped);
        std::thread::Builder::new()
            .name("older-daemon".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    if serve(stream) {
                        break;
                    }
                }
                let _ = std::fs::remove_file(&socket);
                done.store(true, Ordering::Release);
            })
            .map_err(|error| error.to_string())?;
        Ok(Older { stopped })
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}

/// Serves one client until it hangs up: its hello welcomed with another engine, a shutdown
/// done, anything else refused; whether it asked for the shutdown.
fn serve(mut stream: UnixStream) -> bool {
    let mut decoder = Decoder::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => return false,
            Ok(n) => n,
        };
        decoder.feed(&buf[..n]);
        while let Ok(Some(frame)) = decoder.next_frame() {
            let (answer, shutdown) = match Message::decode(frame) {
                Ok(Message::Hello(_)) => (Message::Welcome(welcome()), false),
                Ok(Message::Request(Request { req, body })) => {
                    let shutdown = matches!(body, RequestBody::Shutdown);
                    let outcome = if shutdown {
                        Outcome::Ok(ResponseBody::Done)
                    } else {
                        Outcome::Err(ProtocolError::new(
                            ErrorCode::DaemonVersionMismatch,
                            "the stand-in for an older daemon only stops",
                        ))
                    };
                    (Message::Response(Response { req, outcome }), shutdown)
                }
                _ => continue,
            };
            let mut out = Vec::new();
            if answer.encode(&mut out).is_err() || stream.write_all(&out).is_err() {
                return false;
            }
            if shutdown {
                return true;
            }
        }
    }
}

fn welcome() -> Welcome {
    Welcome {
        protocol: PROTOCOL,
        daemon_version: "0.0.1".into(),
        engine_commit: "0000000000000000000000000000000000000000".into(),
        snapshot_format: SNAPSHOT_FORMAT,
        client_id: 1,
        actor: Actor::system(),
        features: Vec::new(),
    }
}
