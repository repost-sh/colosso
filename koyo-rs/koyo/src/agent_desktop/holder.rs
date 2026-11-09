//! The session holder, `koyo desk-holder` (`docs/agent-desktop.md` 6): started from the app's
//! own executable through `koyo-daemon`, in `<data>/desk-holder/`, as the agent host is.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use koyo_daemon::DaemonDir;
use koyo_desk_wire::holder::{HolderAsk, HolderStatus, SessionState, exchange};

use super::Outcome;
use crate::settings;

/// How long a login may take: a fresh account's first one runs its setup.
const LOGIN_WITHIN: Duration = Duration::from_secs(90);

fn dir() -> Option<DaemonDir> {
    settings::data_dir()
        .map(|data| DaemonDir::new(data.join(koyo_desk_holder::NAME), koyo_desk_holder::NAME))
}

fn ask_on(stream: &mut UnixStream, ask: &HolderAsk) -> std::io::Result<HolderStatus> {
    stream.set_read_timeout(Some(Duration::from_secs(45)))?;
    exchange(stream, ask)
}

/// Asks the holder, if one runs; none starts for it.
pub fn ask_running(ask: &HolderAsk) -> Option<HolderStatus> {
    let dir = dir()?;
    let mut stream = koyo_daemon::connect(&dir.socket()).ok()?;
    ask_on(&mut stream, ask)
        .inspect_err(|error| log::warn!("agent desktop: the holder didn't answer: {error}"))
        .ok()
}

/// Starts the holder when none runs, and waits until it holds the session or says why not.
pub fn start(program: Option<PathBuf>) -> Outcome {
    let (Some(dir), Some(program)) = (dir(), program) else {
        return Outcome::failed("no data folder or no executable to run the holder from");
    };
    let mut stream = match koyo_daemon::connect_or_start(&dir, || {
        let mut command = Command::new(&program);
        command.arg(koyo_desk_holder::NAME);
        command
    }) {
        Ok(stream) => stream,
        Err(error) => return Outcome::failed(format!("the holder didn't start: {error}")),
    };
    let started = Instant::now();
    while started.elapsed() < LOGIN_WITHIN {
        match ask_on(&mut stream, &HolderAsk::Status) {
            Ok(HolderStatus {
                state: SessionState::Held { .. },
                screen,
                ..
            }) => {
                let screen = screen.map_or_else(String::new, |size| {
                    format!(", its screen {}×{}", size.width, size.height)
                });
                return Outcome::done(format!(
                    "The agent session is held{screen}, after {:.1} s.",
                    started.elapsed().as_secs_f64()
                ));
            }
            Ok(HolderStatus {
                state: SessionState::Failed { message },
                ..
            }) => return Outcome::failed(message),
            Ok(HolderStatus {
                state: SessionState::Ended,
                ..
            }) => return Outcome::failed("the session ended as it started"),
            Ok(_) => std::thread::park_timeout(Duration::from_millis(250)),
            Err(error) => return Outcome::failed(format!("the holder went: {error}")),
        }
    }
    Outcome::failed(format!(
        "the agent account wasn't logged in within {} s",
        LOGIN_WITHIN.as_secs()
    ))
}
