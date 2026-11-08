//! The check's pages, served on a local port of its own.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use crate::visual::Result;

/// Serves the check's pages on a local port: a first one whose picture comes late, with a
/// link to a second.
pub(super) fn serve() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream));
        }
    });
    Ok(port)
}

const FIRST: &str = "<!doctype html><title>First page</title><style>\
    body{font:16px -apple-system,sans-serif;margin:0;padding:32px 40px;color:#1d1d1f}\
    h1{font-size:28px;margin:0 0 12px}p{max-width:620px;line-height:1.5}\
    a{position:absolute;left:40px;top:220px;font-size:20px}\
    input{position:absolute;left:40px;top:270px;font-size:16px;padding:6px 10px;width:280px}\
    img{position:absolute;left:40px;top:330px}</style>\
    <h1>A page served for the browser check</h1>\
    <p>This pane runs Chromium in an engine of its own. Find has a few matches here: \
    browser, browser and browser.</p><a href=/second>Go to the second page</a>\
    <input placeholder='Type here'><img src=/late.svg width=320 height=120>";

const SECOND: &str = "<!doctype html><title>Second page</title>\
    <body style='font:16px -apple-system,sans-serif;padding:32px 40px'>\
    <h1>The second page</h1><p>Reached by a click on the first page's link.</p>";

const LATE: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='320' height='120'>\
    <rect width='320' height='120' rx='14' fill='#5e5ce6'/></svg>";

fn answer(mut stream: TcpStream) {
    let mut head = Vec::new();
    let mut chunk = [0u8; 4096];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head);
    let (kind, body) = match head.split_whitespace().nth(1).unwrap_or("/") {
        "/first" => ("text/html", FIRST),
        "/second" => ("text/html", SECOND),
        "/play" => ("text/html", super::play::PLAY),
        "/agent" => ("text/html", super::agent::AGENT),
        "/fold" => ("text/html", super::fold::FOLD),
        "/late.svg" => {
            late(Duration::from_millis(2500));
            ("image/svg+xml", LATE)
        }
        _ => ("text/plain", ""),
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Holds the answer back, so the page is caught loading.
#[expect(
    clippy::disallowed_methods,
    reason = "a server thread of the check's own, late on purpose"
)]
fn late(by: Duration) {
    std::thread::sleep(by);
}
