//! Chrome `DevTools` for a page: the inspector frontend bundled with Chromium,
//! loaded in a page of its own, a pane like any other, and joined to the page's
//! `DevTools` session over a `WebSocket` the host serves on the loopback
//! interface.
//!
//! CEF's own `DevTools` are a window of Chromium's, which is no pane of Koyo's, and
//! Chromium's remote debugging port would open
//! every page to any local process. The bridge here serves one page to one
//! frontend: an ephemeral port, a path only the frontend is told, and the
//! page's in-process session (`SendDevToolsMessage`) behind it.
//!
//! Each frontend gets a session of its own on the page, attached through
//! that root session, as Chrome gives each `DevTools` window. Closing it
//! detaches the session, which turns off everything the frontend turned
//! on, so the next frontend starts clean: console messages logged so far
//! are replayed to it, breakpoints are gone. The root session stays free
//! for other clients. The host's own commands in the frontend's session
//! are told apart by their message ids.
//!
//! The frontend's own controls (its dock menu, its ×, its device toolbar
//! toggle) are on, and a shim in its page passes what they ask to the app
//! (`embedder`, `docs/browser.md` 13).
use crate::blocking::{self, Blocking};
use crate::host;
use cef::{
    Browser, DevToolsMessageObserver, ImplDevToolsMessageObserver, WrapDevToolsMessageObserver,
    rc::Rc, wrap_dev_tools_message_observer,
};
use koyo_browser_ipc::PaneId;
use koyo_browser_ipc::client::token;
use std::io::{self, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::os::fd::AsRawFd as _;
use std::os::raw::c_int;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

pub(crate) mod embedder;
pub(crate) mod message;
mod session;
#[cfg(test)]
mod tests;

pub(crate) use session::{Fields, Inspected, host_id, object};

/// How long an accepted connection may take to say hello.
const HANDSHAKE: Duration = Duration::from_secs(5);

/// The frontend's address for `bridge` (`embedder`).
pub(crate) fn frontend_url(bridge: &Bridge) -> String {
    embedder::address(bridge.port, &bridge.token)
}

/// Starts listening to `page`'s `DevTools` protocol messages; dropping the
/// registration stops them.
pub(crate) fn observe(page: PaneId, host: &cef::BrowserHost) -> Option<cef::Registration> {
    use cef::ImplBrowserHost;
    let mut observer = PageObserver::new(page);
    host.add_dev_tools_message_observer(Some(&mut observer))
}

/// The `WebSocket` the frontend connects to.
pub(crate) struct Bridge {
    port: u16,
    token: String,
    to_frontend: Sender<String>,
    /// Wakes the connection, which sleeps until something happens, to send what it's handed;
    /// let go with the bridge, it ends the connection.
    wake: UnixStream,
    stop: Arc<AtomicBool>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

impl Bridge {
    /// Serves the `DevTools` pane `pane` on a fresh loopback port.
    pub(crate) fn serve(pane: PaneId) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = token()?;
        let (to_frontend, outgoing) = mpsc::channel();
        let (wake, woken) = UnixStream::pair()?;
        wake.set_nonblocking(true)?;
        woken.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let path = format!("/{token}");
        let stopping = stop.clone();
        blocking::thread(&format!("devtools-{pane}"), move |may| {
            let ends = Ends {
                outgoing,
                woken,
                stop: stopping,
            };
            serve(may, pane, &listener, &path, &ends);
        })?;
        Ok(Self {
            port,
            token,
            to_frontend,
            wake,
            stop,
        })
    }

    /// Hands `message` to the frontend, when one is connected. A full wake socket has woken the
    /// connection already.
    fn send(&self, message: String) {
        if self.to_frontend.send(message).is_ok() {
            let _ = (&self.wake).write(&[0]);
        }
    }
}

struct Ends {
    /// What the page hands the frontend.
    outgoing: Receiver<String>,
    /// Readable once the bridge hands something on, and at its end once the bridge is gone.
    woken: UnixStream,
    stop: Arc<AtomicBool>,
}

/// Accepts the frontend, one connection at a time, until stopped.
fn serve(may: Blocking<'_>, pane: PaneId, listener: &TcpListener, path: &str, ends: &Ends) {
    while !ends.stop.load(Ordering::Relaxed) {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        if ends.stop.load(Ordering::Relaxed) {
            return;
        }
        while ends.outgoing.try_recv().is_ok() {}
        if let Some(socket) = handshake(stream, path) {
            connection(may, pane, socket, ends);
        }
    }
}

/// The `WebSocket` handshake, refusing any path but the frontend's.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's refusal is a whole HTTP response"
)]
fn handshake(stream: TcpStream, path: &str) -> Option<WebSocket<TcpStream>> {
    stream.set_read_timeout(Some(HANDSHAKE)).ok()?;
    let socket = tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
        only_path(path, request, response)
    })
    .ok()?;
    socket.get_ref().set_read_timeout(None).ok()?;
    socket.get_ref().set_nonblocking(true).ok()?;
    Some(socket)
}

type Request = tungstenite::handshake::server::Request;
type Response = tungstenite::handshake::server::Response;
type ErrorResponse = tungstenite::handshake::server::ErrorResponse;

/// Accepts the handshake only at `path`.
#[expect(
    clippy::result_large_err,
    reason = "tungstenite's refusal is a whole HTTP response"
)]
fn only_path(path: &str, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    if request.uri().path() == path {
        Ok(response)
    } else {
        let mut refused = ErrorResponse::new(None);
        *refused.status_mut() = tungstenite::http::StatusCode::NOT_FOUND;
        Err(refused)
    }
}

/// One frontend connection: what it sends goes to the page on the UI
/// thread, what the page answers goes back to it. Between the two it
/// sleeps, waking for nothing while nothing happens (`wait`).
fn connection(may: Blocking<'_>, pane: PaneId, mut socket: WebSocket<TcpStream>, ends: &Ends) {
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => host::from_frontend(pane, text.to_string()),
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(e) if would_block(&e) => {
                let Some(unsent) = send_waiting(&mut socket, &ends.outgoing) else {
                    return;
                };
                if !wait(may, socket.get_ref(), &ends.woken, unsent) {
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let _ = socket.close(None);
}

fn would_block(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(e) if e.kind() == io::ErrorKind::WouldBlock)
}

/// Writes out what the page handed the frontend: whether some of it is
/// still to go once the socket takes more, or none when the connection is
/// over.
fn send_waiting(socket: &mut WebSocket<TcpStream>, outgoing: &Receiver<String>) -> Option<bool> {
    loop {
        match outgoing.try_recv() {
            Ok(message) => match socket.write(Message::Text(message.into())) {
                Ok(()) => {}
                Err(e) if would_block(&e) => {}
                Err(_) => return None,
            },
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => return None,
        }
    }
    match socket.flush() {
        Ok(()) => Some(false),
        Err(e) if would_block(&e) => Some(true),
        Err(_) => None,
    }
}

/// Sleeps until the frontend sends something, the bridge hands something
/// on (`woken`), or, while some is `unsent`, the socket takes more: whether
/// the connection goes on, which it doesn't once the bridge is gone.
fn wait(_: Blocking<'_>, socket: &TcpStream, mut woken: &UnixStream, unsent: bool) -> bool {
    let out = if unsent { libc::POLLOUT } else { 0 };
    let mut ready = [
        libc::pollfd {
            fd: socket.as_raw_fd(),
            events: libc::POLLIN | out,
            revents: 0,
        },
        libc::pollfd {
            fd: woken.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // SAFETY: two `pollfd`s, of descriptors the connection holds open meanwhile.
    if unsafe { libc::poll(ready.as_mut_ptr(), 2, -1) } < 0 {
        return io::Error::last_os_error().kind() == io::ErrorKind::Interrupted;
    }
    if ready[1].revents == 0 {
        return true;
    }
    let mut wakes = [0; 64];
    loop {
        match woken.read(&mut wakes) {
            Ok(0) => return false,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return e.kind() == io::ErrorKind::WouldBlock,
        }
    }
}

/// Sends `message` to the frontend of `bridge`.
pub(crate) fn to_frontend(bridge: &Bridge, message: String) {
    bridge.send(message);
}

wrap_dev_tools_message_observer! {
    struct PageObserver {
        page: PaneId,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_message(
            &self,
            _browser: Option<&mut Browser>,
            message: Option<&[u8]>,
        ) -> c_int {
            if let Some(text) = message.and_then(|m| std::str::from_utf8(m).ok()) {
                host::from_page(self.page, text);
            }
            1
        }
    }
}
