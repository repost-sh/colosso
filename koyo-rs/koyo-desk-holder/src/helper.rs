//! The root helper's daemon, as the holder asks it (`docs/agent-desktop.md` 5.4): one JSON line
//! each way, and the lease, a connection held open for as long as the session is held.

use std::io;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use koyo_desk_wire::helper::{HelperAnswer, HelperAsk};
use koyo_desk_wire::holder::exchange;
use koyo_desk_wire::paths;

/// How long the helper may take: ending a session waits for its processes to drain.
const ANSWER_WITHIN: Duration = Duration::from_secs(40);

fn connect() -> io::Result<UnixStream> {
    let stream = UnixStream::connect(paths::HELPER_SOCKET).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("the agent desktop's helper isn't running ({error}); set it up in Settings → Agent desktop"),
        )
    })?;
    stream.set_read_timeout(Some(ANSWER_WITHIN))?;
    Ok(stream)
}

/// Asks once, on a connection of its own.
pub fn ask(ask: &HelperAsk) -> io::Result<HelperAnswer> {
    exchange(&mut connect()?, ask)
}

/// Asks, and turns a refusal into an error saying why.
pub fn must(what: &HelperAsk) -> io::Result<HelperAnswer> {
    match ask(what)? {
        HelperAnswer::Failed { message, .. } => Err(io::Error::other(message)),
        answer => Ok(answer),
    }
}

/// The lease: its connection, which the daemon watches. Dropped, the daemon ends the session.
pub struct Lease {
    _stream: UnixStream,
}

pub fn lease() -> io::Result<Lease> {
    let mut stream = connect()?;
    if let HelperAnswer::Failed { message, .. } = exchange(&mut stream, &HelperAsk::Hold)? {
        return Err(io::Error::other(message));
    }
    // Nothing more is read from it; it only has to stay open.
    stream.set_read_timeout(None)?;
    Ok(Lease { _stream: stream })
}
