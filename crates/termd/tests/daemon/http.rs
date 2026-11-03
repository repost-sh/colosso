//! The app's MCP over HTTP against the daemon (`docs/app-control.md` 6, `docs/responsive.md`
//! 3.1): each session's terminal calls go to the daemon on a connection of its own, which
//! ends with the session, so what the session pinned goes when its client ends it and when
//! the server stops.

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use koyo_term_control::catalog::Scope;
use koyo_term_control::{ClientKind, DaemonCommand, EventKind, Handle, Pinned};
use koyo_term_mcp::Config;
use koyo_term_mcp::http::HttpServer;
use serde_json::{Value, json};

use super::{Client, DAEMON, StopOnDrop, connect, test_dir};

/// One MCP session over HTTP, as a client speaks it: each request on a connection of its
/// own, with the session's id once `initialize` gave one.
struct Session {
    port: u16,
    id: Option<String>,
    next: u64,
}

/// A response: its status, its `Mcp-Session-Id`, and its body, unchunked.
struct Answer {
    status: u16,
    session: Option<String>,
    body: String,
}

impl Session {
    fn start(port: u16) -> Session {
        let mut session = Session {
            port,
            id: None,
            next: 0,
        };
        let init = json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "http-test", "version": "0" },
        });
        session.ask("initialize", &init);
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let answer = session.send("POST", Some(&initialized));
        assert_eq!(answer.status, 202, "{}", answer.body);
        session
    }

    fn send(&self, method: &str, body: Option<&Value>) -> Answer {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut request = format!(
            "{method} /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nConnection: close\r\n\
             Content-Length: {}\r\n",
            self.port,
            body.len()
        );
        if let Some(id) = &self.id {
            write!(
                request,
                "Mcp-Session-Id: {id}\r\nMcp-Protocol-Version: 2025-11-25\r\n"
            )
            .unwrap();
        }
        request.push_str("\r\n");
        request.push_str(&body);
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let end = (response.windows(4))
            .position(|window| window == b"\r\n\r\n")
            .unwrap();
        let head = String::from_utf8(response[..end].to_vec()).unwrap();
        let mut body = response[end + 4..].to_vec();
        let header = |name: &str| {
            (head.lines())
                .filter_map(|line| line.split_once(':'))
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim().to_owned())
        };
        if header("transfer-encoding").is_some_and(|value| value == "chunked") {
            body = unchunked(&body);
        }
        Answer {
            status: head.split(' ').nth(1).unwrap().parse().unwrap(),
            session: header("mcp-session-id"),
            body: String::from_utf8(body).unwrap(),
        }
    }

    /// Asks `method` and its result, from the JSON or the event stream it came in.
    fn ask(&mut self, method: &str, params: &Value) -> Value {
        self.next += 1;
        let message =
            json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        let answer = self.send("POST", Some(&message));
        assert_eq!(answer.status, 200, "{}", answer.body);
        if self.id.is_none() {
            self.id = answer.session;
        }
        let reply = (answer.body.lines())
            .map(|line| line.strip_prefix("data:").unwrap_or(line).trim())
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .find(|reply| reply["id"] == self.next)
            .unwrap_or_else(|| panic!("no answer in {}", answer.body));
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn call(&mut self, tool: &str, arguments: &Value) -> Value {
        let result = self.ask(
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        );
        assert_ne!(result["isError"], true, "{result}");
        result["structuredContent"].clone()
    }
}

fn unchunked(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line = body
            .windows(2)
            .position(|window| window == b"\r\n")
            .unwrap();
        let size = std::str::from_utf8(&body[..line]).unwrap().trim();
        let size = usize::from_str_radix(size, 16).unwrap();
        if size == 0 {
            return out;
        }
        let start = line + 2;
        out.extend_from_slice(&body[start..start + size]);
        body = &body[start + size + 2..];
    }
}

/// Pins a terminal from `session` at 100x30, and the person watching it sees the pin.
fn pin_from(session: &mut Session, person: &Client, name: &str, cwd: &Path) -> Handle {
    let opened = session.call(
        "terminal_open",
        &json!({ "name": name, "command": ["/bin/sh", "-c", "read line"], "cwd": cwd }),
    );
    let handle = Handle(opened["terminal"]["terminal"].as_str().unwrap().to_owned());
    person.attach(&handle, None);
    let resized = session.call(
        "terminal_resize",
        &json!({ "terminal": handle.0, "cols": 100, "rows": 30, "then": {} }),
    );
    assert_eq!(resized["pinned"]["by"]["kind"], "agent", "{resized}");
    // Past the sizes the person's attaching made, to the pin's.
    person.event_until(&handle, |kind| {
        matches!(
            kind,
            EventKind::Size {
                pinned: Some(_),
                ..
            }
        )
    });
    assert_eq!(pinned(person, &handle).unwrap().cols, 100);
    handle
}

/// Waits until the person watching `handle` hears its pin went.
fn unpinned(person: &Client, handle: &Handle) {
    person.event_until(handle, |kind| {
        matches!(kind, EventKind::Size { pinned: None, .. })
    });
    assert_eq!(pinned(person, handle), None);
}

fn pinned(person: &Client, handle: &Handle) -> Option<Pinned> {
    (person.list().into_iter())
        .find(|summary| summary.record.handle == *handle)
        .unwrap()
        .record
        .pinned
}

#[test]
fn an_http_sessions_pin_goes_when_its_client_ends_it_or_the_server_stops() {
    let dir = test_dir("http");
    let _stop = StopOnDrop(dir.clone());
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let config = Config {
        dir,
        daemon: DaemonCommand::binary(DAEMON),
        grants: Scope::AGENT.to_vec(),
        cwd: Some(checkout),
        session: None,
        client: None,
    };
    // No app runs in this test: only the terminals' tools are called.
    let connect =
        koyo_term_mcp::socket_backend(Err(koyo_app_control::ControlError::app_not_running()));
    let server = HttpServer::start(0, config, connect).unwrap();

    // The client ends its session, as MCP has it: `DELETE` with the session's id.
    let mut session = Session::start(server.port());
    let handle = pin_from(&mut session, &person, "ended", &cwd);
    let ended = session.send("DELETE", None);
    assert!((200..300).contains(&ended.status), "{}", ended.body);
    unpinned(&person, &handle);

    // The person turns the server off with a session still open.
    let mut session = Session::start(server.port());
    let handle = pin_from(&mut session, &person, "stopped", &cwd);
    drop(server);
    unpinned(&person, &handle);
}
