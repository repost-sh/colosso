//! Chrome `DevTools` for a page: the inspector frontend bundled with Chromium,
//! loaded in a windowless browser of its own and joined to the page's
//! `DevTools` session over a `WebSocket` the host serves on the loopback
//! interface.
//!
//! CEF's own `DevTools` window cannot render off-screen (the Chrome bootstrap
//! logs "Windowless rendering is not supported for this `DevTools` window" and
//! makes a native window), and Chromium's remote debugging port would open
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
use crate::host;
use cef::{
    Browser, DevToolsMessageObserver, ImplDevToolsMessageObserver, WrapDevToolsMessageObserver,
    rc::Rc, wrap_dev_tools_message_observer,
};
use colosso_browser_ipc::PaneId;
use colosso_browser_ipc::client::token;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::os::raw::c_int;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

mod session;

pub(crate) use session::{Fields, Inspected, host_id, object};

/// How long an accepted connection may take to say hello.
const HANDSHAKE: Duration = Duration::from_secs(5);
/// How long the connection loop waits for something to send when nothing
/// was read.
const IDLE: Duration = Duration::from_millis(5);

/// The frontend's address for `bridge`.
pub(crate) fn frontend_url(bridge: &Bridge) -> String {
    format!(
        "devtools://devtools/bundled/devtools_app.html?ws=127.0.0.1:{}/{}",
        bridge.port, bridge.token
    )
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
    stop: Arc<AtomicBool>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wakes the serving thread from its accept, so it sees the stop.
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
        let stop = Arc::new(AtomicBool::new(false));
        let path = format!("/{token}");
        let stopping = stop.clone();
        std::thread::Builder::new()
            .name(format!("devtools-{pane}"))
            .spawn(move || serve(pane, &listener, &path, &outgoing, &stopping))?;
        Ok(Self {
            port,
            token,
            to_frontend,
            stop,
        })
    }

    /// Hands `message` to the frontend, when one is connected.
    fn send(&self, message: String) {
        let _ = self.to_frontend.send(message);
    }
}

/// Accepts the frontend, one connection at a time, until stopped.
fn serve(
    pane: PaneId,
    listener: &TcpListener,
    path: &str,
    outgoing: &Receiver<String>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        // Messages for a frontend that was not there went nowhere.
        while outgoing.try_recv().is_ok() {}
        if let Some(socket) = handshake(stream, path) {
            connection(pane, socket, outgoing, stop);
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
/// thread, what the page answers goes back to it.
fn connection(
    pane: PaneId,
    mut socket: WebSocket<TcpStream>,
    outgoing: &Receiver<String>,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::Relaxed) {
        match socket.read() {
            Ok(Message::Text(text)) => host::from_frontend(pane, text.to_string()),
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                match outgoing.recv_timeout(IDLE) {
                    Ok(message) => match socket.send(Message::Text(message.into())) {
                        Ok(()) => {}
                        Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                        }
                        Err(_) => return,
                    },
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            Err(_) => return,
        }
    }
    let _ = socket.close(None);
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
            // Handled: the split callbacks are not wanted as well.
            1
        }
    }
}
