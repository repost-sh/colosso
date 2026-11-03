//! Where the socket goes, the fallback for deep data folders, and how the listener claims
//! the socket: private folder and file, a stale socket replaced, a live one left alone.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use koyo_app_control::transport::{BindError, Connection, Listener};
use koyo_app_control::{
    APP_ID, Endpoint, ErrorCode, FALLBACK_SOCKET_FILE, MAX_SOCKET_PATH, fallback_dir,
};

// Marked as test code, as everything here is, so the lints' leave for tests (to unwrap, to
// panic) reaches the helpers too.
/// A fresh, empty folder of the build's own (`target/tmp`).
#[cfg(test)]
fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("ep-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

/// A data folder whose socket path is longer than the limit.
#[cfg(test)]
fn deep_data_dir(root: &Path) -> PathBuf {
    let deep = root.join("a-data-folder-nested-deep-enough".repeat(2));
    assert!(deep.join("control/koyo-control.sock").as_os_str().len() > MAX_SOCKET_PATH);
    deep
}

#[test]
fn the_app_folder_is_the_one_the_terminals_use() {
    assert_eq!(APP_ID, koyo_term_control::APP_ID);
}

#[test]
fn a_short_data_folder_keeps_its_socket_in_its_control_folder() {
    let root = temp_dir("short");
    let data = root.join("data");
    let endpoint = Endpoint::for_data_dir_in(&data, &root.join("tmp")).unwrap();
    assert_eq!(
        endpoint.path(),
        data.join("control").join("koyo-control.sock")
    );
}

#[test]
fn a_deep_data_folder_gets_a_private_folder_under_the_temporary_one() {
    let root = temp_dir("deep");
    let temp = root.join("t");
    let data = deep_data_dir(&root);
    let endpoint = Endpoint::for_data_dir_in(&data, &temp).unwrap();
    let folder = endpoint.path().parent().unwrap();
    assert_eq!(folder, fallback_dir(&data, &temp));
    assert_eq!(endpoint.path().file_name().unwrap(), FALLBACK_SOCKET_FILE);
    assert!(endpoint.path().as_os_str().len() <= MAX_SOCKET_PATH);
    let name = folder.file_name().unwrap().to_str().unwrap();
    let uid = fs::metadata(&root).unwrap().uid();
    let hash = name.strip_prefix(&format!("koyo-{uid}-")).unwrap();
    assert_eq!(hash.len(), 8);
    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));

    // The same folder, however it's written, gets the same name; another folder another.
    let mut slashed = data.clone().into_os_string();
    slashed.push("/");
    assert_eq!(fallback_dir(Path::new(&slashed), &temp), folder);
    assert_ne!(fallback_dir(&data.join("other"), &temp), folder);

    // Binding makes the folder, private, with the socket in it.
    fs::create_dir_all(&temp).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(mode(folder), 0o700);
    assert_eq!(mode(endpoint.path()), 0o600);
    drop(listener);
    assert!(!endpoint.path().exists());
}

#[test]
fn a_fallback_folder_others_may_enter_is_refused() {
    let root = temp_dir("open");
    let temp = root.join("t");
    let data = deep_data_dir(&root);
    let folder = fallback_dir(&data, &temp);
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
    let error = Endpoint::for_data_dir_in(&data, &temp).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(error.message.contains("mode 755"), "{}", error.message);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(Endpoint::for_data_dir_in(&data, &temp).is_ok());
}

#[test]
fn a_fallback_path_too_long_as_well_is_refused() {
    let root = temp_dir("long");
    let data = deep_data_dir(&root);
    let error = Endpoint::for_data_dir_in(&data, &data).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn the_listener_makes_its_folder_and_socket_private() {
    let root = temp_dir("bind");
    let data = root.join("data");
    let endpoint = Endpoint::for_data_dir_in(&data, &root).unwrap();
    let listener = Listener::bind(&endpoint).unwrap();
    assert_eq!(mode(&data.join("control")), 0o700);
    assert_eq!(mode(endpoint.path()), 0o600);
    assert_eq!(listener.path(), endpoint.path());
    assert!(Connection::connect(&endpoint).is_ok());
}

#[test]
fn a_live_socket_is_left_to_its_app() {
    let root = temp_dir("live");
    let endpoint = Endpoint::for_data_dir_in(&root.join("data"), &root).unwrap();
    let first = Listener::bind(&endpoint).unwrap();
    assert!(matches!(
        Listener::bind(&endpoint),
        Err(BindError::InUse(_))
    ));
    assert!(endpoint.path().exists());
    drop(first);
    assert!(!endpoint.path().exists());
}

#[test]
fn a_stale_socket_is_replaced() {
    let root = temp_dir("stale");
    let endpoint = Endpoint::for_data_dir_in(&root.join("data"), &root).unwrap();
    fs::create_dir_all(endpoint.path().parent().unwrap()).unwrap();
    // A socket file nothing answers on, as a crashed app leaves behind.
    drop(UnixListener::bind(endpoint.path()).unwrap());
    assert!(endpoint.path().exists());
    let _listener = Listener::bind(&endpoint).unwrap();
    assert!(Connection::connect(&endpoint).is_ok());
}

#[test]
fn a_file_that_isnt_a_socket_is_never_removed() {
    let root = temp_dir("file");
    let endpoint = Endpoint::for_data_dir_in(&root.join("data"), &root).unwrap();
    fs::create_dir_all(endpoint.path().parent().unwrap()).unwrap();
    fs::write(endpoint.path(), "keep me").unwrap();
    assert!(matches!(Listener::bind(&endpoint), Err(BindError::Io(_))));
    assert_eq!(fs::read_to_string(endpoint.path()).unwrap(), "keep me");
}
