//! What the client's tests share: a host's folder of the test's own, and a stand-in host that
//! answers line by line as the host does.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use koyo_agent::protocol::wire::HOST_VERSION;
use koyo_daemon::DaemonDir;
use serde_json::{Value, json};

/// A host's folder of the test's own, under the checkout's ignored `.superpowers/t`, its socket
/// right in it: reached by the way from the package however deep the checkout is
/// (`koyo_daemon::socket_address`).
pub fn scratch() -> DaemonDir {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let made = MADE.fetch_add(1, Ordering::Relaxed);
    let checkout = std::fs::canonicalize(env!("KOYO_CHECKOUT")).unwrap();
    let root = checkout
        .join(".superpowers/t")
        .join(format!("ra{}-{made}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    DaemonDir::new(root, "host")
}

/// One connection to the stand-in host, from its side.
pub struct Host {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
}

impl Host {
    pub fn accept(listener: &UnixListener) -> Host {
        let (stream, _) = listener.accept().unwrap();
        Host {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    /// The next request the client wrote.
    pub fn request(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        assert!(line.ends_with('\n'), "a request is a line: {line:?}");
        serde_json::from_str(&line).unwrap()
    }

    pub fn send(&mut self, line: &Value) {
        writeln!(self.writer, "{line}").unwrap();
    }

    /// Answers the hello the client says first, which says it's a client of `kind`, as a host
    /// of `version`, busy or not.
    pub fn greet_as(&mut self, kind: &str, version: u32, busy: bool) {
        let hello = self.request();
        assert_eq!(hello["method"], "hello");
        assert_eq!(
            hello["params"],
            json!({ "client": { "kind": kind }, "version": HOST_VERSION })
        );
        let result = json!({ "version": version, "pid": 4242, "busy": busy });
        self.send(&json!({ "id": hello["id"], "result": result }));
    }

    /// Answers the shutdown the client asks for next.
    pub fn answer_shutdown(&mut self) {
        let shutdown = self.request();
        assert_eq!(shutdown["method"], "shutdown");
        self.send(&json!({ "id": shutdown["id"], "result": {} }));
    }

    /// Answers the hello as an idle host.
    pub fn greet(&mut self, kind: &str, version: u32) {
        self.greet_as(kind, version, false);
    }
}

/// A stand-in host listening in `dir`, serving one connection with `serve`, then gone.
pub fn stand_in(dir: &DaemonDir, serve: impl FnOnce(Host) + Send + 'static) -> JoinHandle<()> {
    stand_in_for(dir, vec![Box::new(serve)])
}

/// A stand-in host listening in `dir`, serving each of `connections` in turn, then gone.
pub fn stand_in_for(
    dir: &DaemonDir,
    connections: Vec<Box<dyn FnOnce(Host) + Send>>,
) -> JoinHandle<()> {
    let listener = koyo_daemon::bind(&dir.socket()).unwrap();
    let socket = dir.socket();
    std::thread::spawn(move || {
        for serve in connections {
            serve(Host::accept(&listener));
        }
        drop(listener);
        let _ = std::fs::remove_file(socket);
    })
}
