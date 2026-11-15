//! Where no page is shown in Koyo's window: in tests, which start no engine in their own
//! process. The same call as `browser/native`.

use std::io;

use koyo_browser_ipc::client::{Connection, Launch, Sink};

/// Starts a host process of its own on the engine `launch` names, whose pages nothing shows:
/// what a test drives pages on.
pub fn start(launch: &Launch, sink: impl Sink) -> io::Result<Connection> {
    Connection::start(launch, sink)
}
