use std::io;
use std::net::TcpStream;
use std::time::Duration;

use tungstenite::{Message, WebSocket};

use super::Bridge;

/// A bridge, and a frontend connected to it as the inspector connects.
fn connected() -> (Bridge, WebSocket<TcpStream>) {
    let bridge = Bridge::serve(7).unwrap();
    let stream = TcpStream::connect(("127.0.0.1", bridge.port)).unwrap();
    let address = format!("ws://127.0.0.1:{}/{}", bridge.port, bridge.token);
    let (frontend, _) = tungstenite::client(address, stream).unwrap();
    (bridge, frontend)
}

/// What `frontend` reads within `time`, none when nothing came.
fn read_within(frontend: &mut WebSocket<TcpStream>, time: Duration) -> Option<Message> {
    frontend.get_ref().set_read_timeout(Some(time)).unwrap();
    match frontend.read() {
        Ok(message) => Some(message),
        Err(tungstenite::Error::Io(e))
            if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
        {
            None
        }
        Err(e) => panic!("the connection failed: {e}"),
    }
}

/// The connection sleeps while nothing happens, and what the page hands the frontend then wakes
/// it and reaches the frontend at once.
#[test]
fn what_the_page_hands_an_idle_frontend_reaches_it() {
    let (bridge, mut frontend) = connected();
    assert_eq!(read_within(&mut frontend, Duration::from_millis(200)), None, "idle");
    bridge.send(r#"{"id":1}"#.into());
    let read = read_within(&mut frontend, Duration::from_secs(5));
    assert_eq!(read, Some(Message::Text(r#"{"id":1}"#.into())));
}

/// A message bigger than the socket takes at once goes out whole as the frontend reads it, with
/// nothing after it to push it along.
#[test]
fn a_message_bigger_than_the_socket_takes_reaches_the_frontend_whole() {
    let (bridge, mut frontend) = connected();
    let big = "x".repeat(8 << 20);
    bridge.send(big.clone());
    let read = read_within(&mut frontend, Duration::from_secs(5));
    assert_eq!(read, Some(Message::Text(big.into())));
}

/// The bridge let go, as the `DevTools` pane closes, the connection ends.
#[test]
fn the_connection_ends_with_the_bridge() {
    let (bridge, mut frontend) = connected();
    assert_eq!(read_within(&mut frontend, Duration::from_millis(200)), None, "idle");
    drop(bridge);
    let read = read_within(&mut frontend, Duration::from_secs(5));
    assert!(matches!(read, Some(Message::Close(_))), "closed: {read:?}");
}
