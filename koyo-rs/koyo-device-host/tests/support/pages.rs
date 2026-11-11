//! The touch-lab pages on loopback, as the proof of concept served them: a page reports what it
//! was given with a request for `/log?<json>`, and the log is the record.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

pub struct Pages {
    pub port: u16,
    reports: Arc<Mutex<Vec<Value>>>,
}

fn fixtures() -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", "web"]
        .iter()
        .collect()
}

/// `%xx` and `+` as the characters they stand for.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or("");
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    at += 3;
                    continue;
                }
                out.push(b'%');
            }
            byte => out.push(byte),
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Pages {
    pub fn serve() -> Pages {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("its address").port();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&reports);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let kept = Arc::clone(&kept);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                    let mut stream = &stream;
                    if let Some(query) = path.strip_prefix("/log?") {
                        if let Ok(report) = serde_json::from_str::<Value>(&unescape(query)) {
                            kept.lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .push(report);
                        }
                        let _ = stream
                            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
                        return;
                    }
                    let name = path.trim_start_matches('/').split('?').next().unwrap_or("");
                    let file = fixtures().join(name);
                    match std::fs::read(&file) {
                        Ok(body) if !name.contains("..") && !name.is_empty() => {
                            let head = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(&body);
                        }
                        _ => {
                            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        }
                    }
                });
            }
        });
        Pages { port, reports }
    }

    pub fn url(&self, page: &str) -> String {
        format!("http://127.0.0.1:{}/{page}", self.port)
    }

    pub fn clear(&self) {
        self.reports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// What the pages reported since the last call, after waiting `time`.
    pub fn reported(&self, time: Duration) -> Vec<Value> {
        let started = Instant::now();
        while started.elapsed() < time {
            std::thread::park_timeout(time.saturating_sub(started.elapsed()));
        }
        std::mem::take(&mut *self.reports.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The first report whose kind is `kind`, waiting up to `limit` for it.
    pub fn wait_for(&self, kind: &str, limit: Duration) -> Option<Value> {
        let started = Instant::now();
        while started.elapsed() < limit {
            let found = self
                .reports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|report| report["kind"] == kind)
                .cloned();
            if found.is_some() {
                return found;
            }
            std::thread::park_timeout(Duration::from_millis(50));
        }
        None
    }
}

/// Opens `page` in the simulator's Safari and waits for it to say it's there: a new simulator's
/// Safari shows its Start Page the first time a link is opened, so it is opened again.
pub fn open(target: &super::Target, pages: &Pages, page: &str, ready: &str) -> Value {
    for _ in 0..3 {
        pages.clear();
        super::simctl(target, &["openurl", &target.udid, &pages.url(page)]);
        if let Some(report) = pages.wait_for(ready, Duration::from_secs(12)) {
            // Settled: Safari's bars stop moving.
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(2) {
                std::thread::park_timeout(Duration::from_millis(100));
            }
            pages.clear();
            return report;
        }
    }
    panic!("{page} didn't load");
}
