//! The host's end of the hello against an app that isn't one: a socket pair whose other end
//! says what the app would.

use std::cell::RefCell;

use koyo_frame::Mismatch;

use super::*;
use crate::sys;

unsafe extern "C" {
    fn CFRunLoopPerformBlock(run_loop: sys::Ref, mode: sys::Ref, block: &block2::Block<dyn Fn()>);
}

/// The app's hello in `protocol`, naming `surfaces`.
fn apps_hello(protocol: u32) -> ToHost {
    ToHost::Hello {
        protocol,
        surfaces: "codes.koyo.desk.test".into(),
    }
}

#[test]
fn an_app_of_another_protocol_is_refused_with_each_side_named() {
    let (host, mut app) = UnixStream::pair().unwrap();
    write_frame(&mut app, &apps_hello(PROTOCOL + 1)).unwrap();
    let refused = hello(&host).expect_err("an app of another protocol was heard");
    let mismatch = refused
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Mismatch>());
    let expected = Mismatch {
        app: PROTOCOL + 1,
        host: PROTOCOL,
    };
    assert_eq!(mismatch, Some(&expected), "{refused}");
}

#[test]
fn the_run_loop_is_served_while_the_host_waits_for_the_apps_hello() {
    let (host, app) = UnixStream::pair().unwrap();
    let app = RefCell::new(app);
    let says_hello = block2::RcBlock::new(move || {
        let _ = write_frame(&mut *app.borrow_mut(), &apps_hello(PROTOCOL));
    });
    // SAFETY: a block for this thread's own run loop, which copies it and runs it there in the
    // default mode, the one the host pumps.
    unsafe {
        CFRunLoopPerformBlock(
            sys::CFRunLoopGetCurrent(),
            sys::kCFRunLoopDefaultMode,
            &says_hello,
        );
    }
    let heard = hello(&host).map_err(|error| error.to_string());
    assert_eq!(
        heard.as_deref(),
        Ok("codes.koyo.desk.test"),
        "the app says hello only once the run loop runs"
    );
}
