//! The link to the agent desktop's host against a host that isn't one: a socket pair whose
//! other end says what a host would.

use koyo_frame::Mismatch;

use super::*;
use crate::blocking::in_test;

#[test]
fn a_host_of_another_protocol_is_refused_before_it_runs() {
    let (app, mut host) = UnixStream::pair().unwrap();
    let hello = FromHost::Hello {
        protocol: PROTOCOL + 1,
        pid: 1,
        accessibility: true,
        screen_recording: true,
    };
    write_frame(&mut host, &hello).unwrap();
    let surfaces = mach::Receiver::check_in("codes.koyo.desk-test").unwrap();
    let sink = futures::channel::mpsc::unbounded().0;
    let linked = in_test(|may| linked(app, surfaces, sink, may));
    let refused = linked
        .err()
        .expect("a host of another protocol counted as running");
    let mismatch = refused.get_ref().and_then(|inner| inner.downcast_ref::<Mismatch>());
    let expected = Mismatch {
        app: PROTOCOL,
        host: PROTOCOL + 1,
    };
    assert_eq!(mismatch, Some(&expected), "{refused}");
}

#[test]
fn a_host_in_this_protocol_runs_with_the_grants_it_said_it_holds() {
    let (app, mut host) = UnixStream::pair().unwrap();
    let hello = FromHost::Hello {
        protocol: PROTOCOL,
        pid: 1,
        accessibility: true,
        screen_recording: false,
    };
    write_frame(&mut host, &hello).unwrap();
    let surfaces = mach::Receiver::check_in("codes.koyo.desk-test").unwrap();
    let sink = futures::channel::mpsc::unbounded().0;
    let (_running, grants) = in_test(|may| linked(app, surfaces, sink, may)).unwrap();
    let expected = Grants {
        accessibility: true,
        screen_recording: false,
    };
    assert_eq!(grants, expected);
    let told = read_frame::<ToHost>(&mut host).unwrap();
    assert!(matches!(told, Some(ToHost::Hello { protocol: PROTOCOL, .. })));
}
