//! A socket in a folder deeper than a socket's address holds, as the checkout's scratch folder
//! of a checkout deep enough is: bound and reached all the same, where it is.

use std::io::{Read as _, Write as _};

use koyo_daemon::{SOCKET_PATH_MAX, bind, connect, socket_address};

#[test]
fn a_socket_too_deep_for_its_address_is_bound_and_reached_where_it_is() {
    // A folder of this test's own in the checkout's scratch folder, deep enough that its socket's
    // absolute path is too long for an address whatever the checkout's, and the way to it from
    // the package, where tests run, isn't.
    let checkout = std::fs::canonicalize(env!("KOYO_CHECKOUT")).unwrap();
    let name = format!("socket-{}-{}", std::process::id(), "d".repeat(60));
    let dir = checkout.join(".superpowers/t").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.sock");
    assert!(
        path.as_os_str().len() > SOCKET_PATH_MAX,
        "{} is short",
        path.display()
    );
    assert!(socket_address(&path).as_os_str().len() <= SOCKET_PATH_MAX);

    let listener = bind(&path).unwrap();
    assert!(
        std::fs::metadata(&path).is_ok(),
        "the socket is in its folder"
    );
    let mut client = connect(&path).unwrap();
    let (mut served, _) = listener.accept().unwrap();
    client.write_all(b"ping").unwrap();
    let mut heard = [0; 4];
    served.read_exact(&mut heard).unwrap();
    assert_eq!(&heard, b"ping");
    std::fs::remove_dir_all(&dir).unwrap();
}
