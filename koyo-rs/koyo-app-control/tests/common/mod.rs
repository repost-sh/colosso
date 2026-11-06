//! A fake app for the socket tests: a listener in the build's temporary folder that welcomes
//! one client and hands the connection to the test's own script.

use std::fs;
use std::path::PathBuf;
use std::thread::JoinHandle;

use koyo_app_control::frame::{Call, ClientFrame, MAX_CLIENT_FRAME, MAX_SERVER_FRAME, ServerFrame};
use koyo_app_control::transport::{FrameReader, FrameWriter, Listener};
use koyo_app_control::{Endpoint, Hello, Scope};

/// A fresh, empty folder of the build's own (`target/tmp`), named short so socket paths in
/// it stay well under the limit.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ac-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Listens in `temp_dir(name)`, welcomes the first client with `grant`, then runs `script`
/// with its hello and the connection. The socket goes when the script returns.
pub fn spawn_app(
    name: &str,
    grant: Vec<Scope>,
    script: impl FnOnce(Hello, FrameReader, FrameWriter) + Send + 'static,
) -> (Endpoint, JoinHandle<()>) {
    let endpoint = Endpoint::at(temp_dir(name).join("app.sock"));
    let listener = Listener::bind(&endpoint).unwrap();
    let app = std::thread::spawn(move || {
        let (mut reader, writer) = listener.accept().unwrap().split().unwrap();
        let Some(ClientFrame::Hello(hello)) = reader.read(MAX_CLIENT_FRAME).unwrap() else {
            panic!("the client didn't say hello first");
        };
        writer
            .write(&ServerFrame::Welcome { grant }, MAX_SERVER_FRAME)
            .unwrap();
        script(hello, reader, writer);
        drop(listener);
    });
    (endpoint, app)
}

/// The client's next frame, which must be a call.
pub fn next_call(reader: &mut FrameReader) -> Call {
    match reader.read(MAX_CLIENT_FRAME).unwrap() {
        Some(ClientFrame::Call(call)) => call,
        other => panic!("expected a call, got {other:?}"),
    }
}
