//! The link to a device's host against a host that isn't one: a socket pair whose other end
//! says what a host would.

use koyo_frame::Mismatch;

use super::*;

#[test]
fn a_host_of_another_protocol_is_refused_before_it_runs() {
    let (app, mut host) = UnixStream::pair().unwrap();
    let hello = FromHost::Hello {
        protocol: PROTOCOL + 1,
    };
    write_frame(&mut host, &hello).unwrap();
    let receiver = mach::Receiver::check_in("codes.koyo.device-test").unwrap();
    let linked = linked(
        app,
        "token".into(),
        receiver,
        futures::channel::mpsc::unbounded().0,
    );
    let refused = linked
        .err()
        .expect("a host of another protocol counted as running");
    let mismatch = refused
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Mismatch>());
    let expected = Mismatch {
        app: PROTOCOL,
        host: PROTOCOL + 1,
    };
    assert_eq!(mismatch, Some(&expected), "{refused}");
}
