//! What the smoke tests share: the engine to start, a session with the host in a process of
//! its own, where its pages are in holders parked off the screen, and a page to open, serve
//! and ask things of.
use koyo_browser_ipc::client::{Connection, Incoming, Launch};
use koyo_browser_ipc::{AppRequest, HostEvent, Viewport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::agents::cdp;

pub fn launch(test: &str) -> Launch {
    let engine = PathBuf::from(
        std::env::var("KOYO_BROWSER_ENGINE")
            .expect("KOYO_BROWSER_ENGINE points at an installed engine"),
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(engine.join("engine.json")).expect("engine.json"))
            .expect("json");
    // Scratch inside the checkout, one root per test so they can run side by
    // side; the host inherits the working directory.
    let scratch = PathBuf::from(format!(
        "../../.superpowers/t/{}-{test}",
        std::process::id()
    ));
    Launch {
        executable: engine.join(manifest["host"].as_str().expect("host path")),
        profiles: scratch.join("profiles"),
        passkeys: false,
    }
}

/// A running host and what it sent so far.
pub struct Session {
    pub host: Connection,
    incoming: std::sync::mpsc::Receiver<Incoming>,
}

impl Session {
    pub fn start(launch: &Launch) -> Self {
        Self::start_with(launch, &[])
    }

    /// A host with `environment` added to its own, as Chromium switches to try are.
    pub fn start_with(launch: &Launch, environment: &[(&str, &str)]) -> Self {
        let (sink, incoming) = std::sync::mpsc::channel();
        let sink = move |message| sink.send(message).err().map(|e| e.0);
        let host = Connection::start_with(launch, environment, sink)
            .expect("the host starts and says hello");
        Self { host, incoming }
    }

    /// Waits for the event `want` accepts, failing on host errors meanwhile.
    pub fn wait(&mut self, what: &str, want: impl FnMut(&HostEvent) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        assert!(self.pump(deadline, want), "timed out waiting for {what}");
    }

    /// Handles what the host sends until `deadline`, or until `want` accepts an event:
    /// whether it did.
    pub fn pump(&mut self, deadline: Instant, mut want: impl FnMut(&HostEvent) -> bool) -> bool {
        while Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(incoming) = self
                .incoming
                .recv_timeout(left.min(Duration::from_millis(500)))
            else {
                continue;
            };
            match incoming {
                Incoming::Event(event) => {
                    if want(&event) {
                        return true;
                    }
                    if let HostEvent::Error { pane, message } = &event {
                        panic!("host error on pane {pane:?}: {message}");
                    }
                }
                Incoming::Closed => panic!("the host went away"),
            }
        }
        false
    }
}

/// The size [`open`] opens a page at.
pub const VIEWPORT: Viewport = Viewport {
    width: 400,
    height: 300,
    scale: 1.,
};

/// Opens `url` as pane 1 and waits for it to load, `title` being its title.
pub fn open(session: &mut Session, url: &str, title: &str) {
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: url.into(),
            viewport: VIEWPORT,
            profile: "smoke".into(),
        })
        .expect("open");
    session.wait("the page to load", |event| {
        matches!(event, HostEvent::Page { pane: 1, state } if state.title == title && !state.loading)
    });
}

/// The JSON `expression` makes in pane 1's page, once `settled` holds of it: asked again every
/// 50 ms of the page's until then, for `within` at most.
pub fn until(
    session: &mut Session,
    expression: &str,
    what: &str,
    within: Duration,
    settled: impl Fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let later = format!("new Promise(done => setTimeout(() => done({expression}), 50))");
    let deadline = Instant::now() + within;
    loop {
        let text = evaluate(session, &later);
        let value = serde_json::from_str(text.as_str().expect("JSON text")).expect("JSON");
        if settled(&value) {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
    }
}

/// What `expression` comes to in pane 1's page, a promise awaited. A page that stops
/// meanwhile fails the test with why.
pub fn evaluate(session: &mut Session, expression: &str) -> serde_json::Value {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let params = serde_json::json!({
        "expression": expression,
        "returnByValue": true,
        "awaitPromise": true,
    });
    cdp(session, call, "Runtime.evaluate", params);
    let mut answer = None;
    session.wait("the page's answer", |event| match event {
        HostEvent::Gone { reason, .. } => panic!("the page stopped: {reason}"),
        HostEvent::CdpReply {
            call: answered,
            result,
            error,
            ..
        } if *answered == call => {
            answer = Some((result.clone(), error.clone()));
            true
        }
        _ => false,
    });
    let (result, error) = answer.expect("answered");
    assert_eq!(error, None, "{expression}");
    let result = result.expect("a result");
    assert!(
        result["exceptionDetails"].is_null(),
        "{expression}: {result}"
    );
    result["result"]["value"].clone()
}

pub fn close(session: Session, launch: &Launch) {
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// Serves `page` at every path of a local address, which a page needs to change its own:
/// the address.
pub fn serve(page: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let address = format!("http://{}/", listener.local_addr().expect("an address"));
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|end| end == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(read) if read > 0 => request.extend_from_slice(&chunk[..read]),
                    _ => break,
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    address
}
