//! A daemon of another build stopped for this one (`docs/terminals.md` 5.1): asked to when
//! it takes the client's hello, signalled through its lock's pid when it refuses it, and its
//! terminals remembered for the daemon started next.

use std::os::unix::net::UnixStream;

use koyo_term_control::client::{inbox, stop_daemon};
use koyo_term_control::{ClientError, ClientKind, Connection, ErrorCode, PROTOCOL};

use super::{StopOnDrop, connect, hello, test_dir};

#[test]
fn a_daemon_that_refuses_the_hello_is_stopped_by_its_pid() {
    let dir = test_dir("upgrade-pid");
    let _stop = StopOnDrop(dir.clone());
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let person = connect(&dir, ClientKind::App);
    let (kept, _) = person.open("kept", &cwd, &["/bin/sh", "-c", "sleep 30"], false);

    // A later build's client, which this daemon refuses.
    let mut later = hello(ClientKind::App, None);
    later.protocol = PROTOCOL + 1;
    let (tx, _inbox) = inbox();
    match Connection::connect(&dir.socket(), &later, tx) {
        Err(ClientError::Protocol(error)) => {
            assert_eq!(error.code, ErrorCode::DaemonVersionMismatch);
        }
        other => panic!("{:?}", other.map(|(_, welcome)| welcome)),
    }
    stop_daemon(&dir, &later).unwrap();
    assert!(UnixStream::connect(dir.socket()).is_err());

    // The next daemon has the terminal, ended by the restart.
    let again = connect(&dir, ClientKind::App);
    let listed = (again.list().into_iter())
        .find(|summary| summary.record.handle == kept.handle)
        .unwrap();
    assert!(listed.record.exit.unwrap().daemon_restart);
}

#[test]
fn a_daemon_that_takes_the_hello_is_asked_to_stop() {
    let dir = test_dir("upgrade-ask");
    let _stop = StopOnDrop(dir.clone());
    let _person = connect(&dir, ClientKind::App);
    stop_daemon(&dir, &hello(ClientKind::App, None)).unwrap();
    assert!(UnixStream::connect(dir.socket()).is_err());
    // With none running, there's nothing to stop.
    stop_daemon(&dir, &hello(ClientKind::App, None)).unwrap();
}
