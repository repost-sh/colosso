//! The VT thread against a daemon that only says welcome: the stream is the test's, so what it
//! sends is the program's output, and what the thread makes of it comes out of the slot.

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedReceiver;
use koyo_term_control::{
    Actor, ClientKind, Connection, DAEMON_VERSION, Hello, Message, Output, PROTOCOL,
    SNAPSHOT_FORMAT, Welcome,
};

use super::{Command, Update, Vt};
use crate::client::Stream;

/// How long a frame that's due may take to come.
const PATIENCE: Duration = Duration::from_secs(10);
/// How long a frame that isn't due is waited for.
const AWHILE: Duration = Duration::from_millis(300);

/// A connection to a daemon of the test's own, which says welcome and nothing more.
fn welcomed() -> Arc<Connection> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-vt");
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join(format!("{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let listener = koyo_daemon::bind(&socket).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hello = [0; 4096];
        let _ = stream.read(&mut hello).unwrap();
        let welcome = Welcome {
            protocol: PROTOCOL,
            daemon_version: DAEMON_VERSION.to_string(),
            engine_commit: koyo_term_engine::GHOSTTY_COMMIT.into(),
            snapshot_format: SNAPSHOT_FORMAT,
            client_id: 1,
            actor: Actor::system(),
            features: Vec::new(),
        };
        let mut out = Vec::new();
        Message::Welcome(welcome).encode(&mut out).unwrap();
        stream.write_all(&out).unwrap();
        let _ = stream.read_to_end(&mut Vec::new());
    });
    let hello = Hello::new(ClientKind::App, "koyo-term tests", "0");
    let (inbox, _) = crossbeam_channel::unbounded();
    Connection::connect(&socket, &hello, inbox).unwrap().0
}

fn output(offset: u64, text: &'static str) -> Stream {
    Stream::Output(Output {
        index: 0,
        offset,
        bytes: text.into(),
    })
}

/// Whether the thread says a frame waits in the slot within `time`.
fn framed(updates: &mut UnboundedReceiver<Update>, time: Duration) -> bool {
    let until = Instant::now() + time;
    while Instant::now() < until {
        match updates.try_recv() {
            Ok(Update::Frame) => return true,
            Ok(_) => {}
            Err(_) => {
                let _ = crossbeam_channel::after(Duration::from_millis(5)).recv();
            }
        }
    }
    false
}

/// Has the thread take in every command sent before this one.
fn settled(vt: &Vt) {
    let (reply, replied) = crossbeam_channel::bounded(1);
    vt.send(Command::Copy(reply));
    replied.recv_timeout(PATIENCE).unwrap();
}

/// Out of sight, the engine takes the program's output in and no frame is made of it; seen
/// again, a frame with all of it goes out at once.
#[test]
fn out_of_sight_output_is_taken_in_and_framed_once_it_is_seen_again() {
    let (program, stream) = crossbeam_channel::unbounded();
    let (said, mut updates) = futures::channel::mpsc::unbounded();
    let vt = Vt::start("test", 0, welcomed(), stream, said).unwrap();

    program.send(output(0, "first")).unwrap();
    assert!(framed(&mut updates, PATIENCE), "a frame of what came in sight");
    assert!(vt.take_frame().unwrap().text().contains("first"));

    vt.send(Command::Seen(false));
    settled(&vt);
    program.send(output(5, "\r\nsecond")).unwrap();
    program.send(output(13, "\r\nthird")).unwrap();
    assert!(!framed(&mut updates, AWHILE), "a frame made out of sight");

    vt.send(Command::Seen(true));
    assert!(framed(&mut updates, PATIENCE), "no frame once it's seen again");
    let text = vt.take_frame().unwrap().text();
    assert!(
        text.contains("first") && text.contains("second") && text.contains("third"),
        "{text:?}"
    );
}
