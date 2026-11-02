//! The MCP server over Streamable HTTP, as the app serves it while the person has it on
//! (`docs/app-control.md` 6): `http://127.0.0.1:<port>/mcp`, on the loopback address only.
//! It runs the stdio relay's handler; each MCP session gets a server of its own, whose app
//! calls go to the backend `connect` makes and whose terminal calls go to the daemon as
//! the session's client. Against DNS rebinding, as the MCP spec asks, a request is refused
//! unless its `Host` is this server's own loopback authority and its `Origin`, when it sends
//! one, is too. Dropping the server stops it and frees the port.

use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt as _, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio_util::sync::CancellationToken;

use crate::{Config, Connect, Server};

/// Where the server answers MCP.
pub const PATH: &str = "/mcp";
/// The port served unless the person picks another; unassigned by IANA, and distinct from
/// monocode's 6262 so both apps serve at once.
pub const DEFAULT_PORT: u16 = 6272;

type Service = StreamableHttpService<Server, LocalSessionManager>;

/// The server's URL on `port`.
pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}{PATH}")
}

/// A running server. Dropping it stops it and frees the port.
pub struct HttpServer {
    port: u16,
    stop: CancellationToken,
    thread: Option<JoinHandle<()>>,
}

impl HttpServer {
    /// Binds `127.0.0.1:port` (`0` takes any free port) and serves MCP there on a thread of
    /// its own. Each session is a [`Server`] made from `config`, its app calls going to the
    /// backend `connect` makes.
    pub fn start(port: u16, config: Config, connect: Connect) -> std::io::Result<HttpServer> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        // Multi-threaded: the catalogs' calls block, and run on the runtime's blocking threads.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mcp-http")
            .enable_all()
            .build()?;
        let stop = CancellationToken::new();
        let serving = stop.clone();
        let thread = std::thread::Builder::new()
            .name("mcp-http".into())
            .spawn(move || {
                runtime.block_on(serve(listener, port, config, connect, serving));
                // Calls still waiting on the person finish on their own.
                runtime.shutdown_background();
            })?;
        Ok(HttpServer {
            port,
            stop,
            thread: Some(thread),
        })
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Its URL.
    pub fn url(&self) -> String {
        url(self.port)
    }
}

impl Drop for HttpServer {
    /// Ends every session and waits until the port is free again.
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::warn!("the MCP server's thread stopped with a panic");
        }
    }
}

/// The rmcp service's settings: loopback `Host` and `Origin` only.
fn service_config(port: u16, stop: &CancellationToken) -> StreamableHttpServerConfig {
    let authorities = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    StreamableHttpServerConfig::default()
        .with_allowed_hosts(authorities.clone())
        .with_allowed_origins(
            authorities
                .iter()
                .map(|authority| format!("http://{authority}")),
        )
        .with_cancellation_token(stop.child_token())
}

async fn serve(
    listener: TcpListener,
    port: u16,
    config: Config,
    connect: Connect,
    stop: CancellationToken,
) {
    let listener = match tokio::net::TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("the MCP server can't listen: {error}");
            return;
        }
    };
    let service: Service = StreamableHttpService::new(
        move || Ok(Server::new(config.clone(), &connect, None)),
        Arc::new(LocalSessionManager::default()),
        service_config(port, &stop),
    );
    while let Some(accepted) = stop.run_until_cancelled(listener.accept()).await {
        let Ok((stream, _)) = accepted else {
            // A failing accept, such as running out of descriptors, mustn't spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        let (service, stop) = (service.clone(), stop.clone());
        tokio::spawn(async move {
            let handler = service_fn(move |request| {
                let service = service.clone();
                async move { Ok::<_, Infallible>(route(&service, request).await) }
            });
            let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), handler);
            if let Some(Err(error)) = stop.run_until_cancelled(connection).await {
                log::debug!("an MCP connection ended: {error}");
            }
        });
    }
}

/// MCP at [`PATH`]; anything else isn't found.
async fn route(
    service: &Service,
    request: Request<Incoming>,
) -> Response<BoxBody<Bytes, Infallible>> {
    if request.uri().path() == PATH {
        return service.handle(request).await;
    }
    let body = Full::new(Bytes::from_static(b"Not Found: Colosso serves MCP at /mcp")).boxed();
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}
